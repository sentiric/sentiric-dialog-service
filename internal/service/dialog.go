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
	return &DialogService{
		stateManager: sm,
		llmClient:    lc,
		log:          log,
	}
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
			// 1. Oturum Kurulumu & Proactive Greeting
			sess, err := s.stateManager.GetSession(ctx, payload.Config.SessionId)
			if err != nil {
				return status.Errorf(codes.Internal, "State error: %v", err)
			}
			sess.UserID = payload.Config.UserId
			currentSession = sess

			if len(sess.History) == 0 {
				l.Info().Msg("✨ Yeni çağrı: Karşılama mesajı üretiliyor...")
				if err := s.generateAndSend(ctx, stream, currentSession, "PROMPT_GREETING", traceID, sentenceBuffer, l); err != nil {
					l.Error().Err(err).Msg("Karşılama mesajı başarısız.")
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

			l.Info().Str("input", userInput).Msg("🗣️ Kullanıcı girdisi işleniyor")
			s.stateManager.AddTurn(currentSession, "user", userInput)

			if err := s.generateAndSend(ctx, stream, currentSession, userInput, traceID, sentenceBuffer, l); err != nil {
				return err
			}
		}
	}
}

// generateAndSend: LLM'den yanıt alır, cümlelere böler ve stream eder.
func (s *DialogService) generateAndSend(
	ctx context.Context,
	stream dialogv1.DialogService_StreamConversationServer,
	sess *state.Session,
	prompt string,
	traceID string,
	sb *SentenceBuffer,
	l zerolog.Logger,
) error {
	tokensChan, err := retry.WithExponentialBackoff(ctx, func(ctx context.Context) (<-chan string, error) {
		return s.llmClient.Generate(ctx, traceID, sess.History, prompt)
	}, 3)

	if err != nil {
		return status.Errorf(codes.Unavailable, "LLM unreachable: %v", err)
	}

	var fullAIResponse strings.Builder

	for token := range tokensChan {
		fullAIResponse.WriteString(token)

		// Cümle bazlı bufferlama mantığı
		if sentence, ok := sb.Push(token); ok {
			s.sendSentence(stream, sentence)
		}
	}

	// Buffer'da kalan son parçayı gönder
	if finalPart, ok := sb.Flush(); ok {
		s.sendSentence(stream, finalPart)
	}

	// Final sinyali ve state kaydı
	stream.Send(&dialogv1.StreamConversationResponse{
		Payload: &dialogv1.StreamConversationResponse_IsFinalResponse{IsFinalResponse: true},
	})

	s.stateManager.AddTurn(sess, "assistant", fullAIResponse.String())
	return s.stateManager.SaveSession(ctx, sess)
}

func (s *DialogService) sendSentence(stream dialogv1.DialogService_StreamConversationServer, text string) {
	stream.Send(&dialogv1.StreamConversationResponse{
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
		l.Warn().Msg("Stream iptal edildi.")
		return nil
	}
	l.Error().Err(err).Msg("Stream hatası.")
	return err
}
