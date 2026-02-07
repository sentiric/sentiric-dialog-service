// sentiric-dialog-service/internal/service/dialog.go
package service

import (
	"context"
	"io"
	"strings"

	"github.com/rs/zerolog"
	dialogv1 "github.com/sentiric/sentiric-contracts/gen/go/sentiric/dialog/v1"
	"github.com/sentiric/sentiric-dialog-service/internal/clients/llm"
	"github.com/sentiric/sentiric-dialog-service/internal/retry"
	"github.com/sentiric/sentiric-dialog-service/internal/state"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
)

type DialogService struct {
	dialogv1.UnimplementedDialogServiceServer
	stateManager *state.Manager
	llmClient    llm.Client
	log          zerolog.Logger
}

func NewDialogService(sm *state.Manager, lc llm.Client, log zerolog.Logger) *DialogService {
	return &DialogService{stateManager: sm, llmClient: lc, log: log}
}

func (s *DialogService) StreamConversation(stream dialogv1.DialogService_StreamConversationServer) error {
	ctx := stream.Context()
	traceID := s.getTraceID(ctx)
	l := s.log.With().Str("trace_id", traceID).Logger()

	var currentSession *state.Session
	var inputBuffer strings.Builder
	sentenceBuffer := NewSentenceBuffer()

	for {
		in, err := stream.Recv()
		if err == io.EOF {
			return nil
		}
		if err != nil {
			return s.handleStreamError(err, l)
		}

		switch payload := in.Payload.(type) {
		case *dialogv1.StreamConversationRequest_Config:
			// 1. Session Retrieval
			sess, err := s.stateManager.GetSession(ctx, payload.Config.SessionId)
			if err != nil {
				l.Error().Err(err).Msg("Redis session lookup failed.")
				return status.Errorf(codes.Internal, "Redis lookup fail")
			}
			sess.UserID = payload.Config.UserId
			currentSession = sess

			// 2. Proactive Engagement
			if len(sess.History) == 0 {
				l.Info().Msg("✨ Initializing context for new session.")
				if err := s.generateAndStream(ctx, stream, currentSession, "PROMPT_GREETING", traceID, sentenceBuffer, l); err != nil {
					l.Error().Err(err).Msg("Greeting sequence failed.")
				}
			}

		case *dialogv1.StreamConversationRequest_TextInput:
			inputBuffer.WriteString(payload.TextInput)

		case *dialogv1.StreamConversationRequest_IsFinalInput:
			if !payload.IsFinalInput || inputBuffer.Len() == 0 {
				continue
			}

			userInput := strings.TrimSpace(inputBuffer.String())
			inputBuffer.Reset()

			l.Info().Str("input", userInput).Msg("🗣️ Processing user utterance.")
			s.stateManager.AddTurn(currentSession, "user", userInput)

			if err := s.generateAndStream(ctx, stream, currentSession, userInput, traceID, sentenceBuffer, l); err != nil {
				return err
			}
		}
	}
}

func (s *DialogService) generateAndStream(
	ctx context.Context,
	stream dialogv1.DialogService_StreamConversationServer,
	sess *state.Session,
	prompt string,
	traceID string,
	sb *SentenceBuffer,
	l zerolog.Logger,
) error {
	l.Debug().Str("prompt", prompt).Msg("🚀 Initiating LLM token stream.")

	// [RETRY MANTIGI]: LLM ulasilamazsa 3 kez dene
	tokensChan, err := retry.WithExponentialBackoff(ctx, func(ctx context.Context) (<-chan string, error) {
		return s.llmClient.Generate(ctx, traceID, sess.History, prompt)
	}, 3)

	if err != nil {
		l.Error().Err(err).Msg("❌ LLM backend reached maximum retry limit.")
		return status.Errorf(codes.Unavailable, "LLM backend unreachable")
	}

	var fullResponse strings.Builder
	tokenCount := 0

	for token := range tokensChan {
		tokenCount++
		fullResponse.WriteString(token)

		// Kelime kelime değil, cümle cümle gönder (TTS kalitesi için)
		if sentence, ok := sb.Push(token); ok {
			l.Trace().Str("sentence", sentence).Msg("📤 Streaming sentence to client.")
			s.sendToClient(stream, sentence)
		}
	}

	// Kalan parçaları süpür
	if finalPart, ok := sb.Flush(); ok {
		l.Trace().Str("final_part", finalPart).Msg("📤 Streaming final sentence part.")
		s.sendToClient(stream, finalPart)
	}

	// İşlem bitti sinyali (EOS)
	l.Info().
		Int("tokens_received", tokenCount).
		Int("history_depth", len(sess.History)).
		Msg("🏁 LLM stream completed successfully.")

	stream.Send(&dialogv1.StreamConversationResponse{
		Payload: &dialogv1.StreamConversationResponse_IsFinalResponse{IsFinalResponse: true},
	})

	// Kaydet
	s.stateManager.AddTurn(sess, "assistant", fullResponse.String())
	return s.stateManager.SaveSession(ctx, sess)
}

func (s *DialogService) sendToClient(stream dialogv1.DialogService_StreamConversationServer, text string) {
	_ = stream.Send(&dialogv1.StreamConversationResponse{
		Payload: &dialogv1.StreamConversationResponse_TextResponse{TextResponse: text + " "},
	})
}

func (s *DialogService) getTraceID(ctx context.Context) string {
	if md, ok := metadata.FromIncomingContext(ctx); ok {
		if ids := md.Get("x-trace-id"); len(ids) > 0 {
			return ids[0]
		}
	}
	return "unknown"
}

func (s *DialogService) handleStreamError(err error, l zerolog.Logger) error {
	if status.Code(err) == codes.Canceled {
		l.Warn().Msg("Client closed dialog stream.")
		return nil
	}
	l.Error().Err(err).Msg("Dialog stream recv error.")
	return err
}
