package service

import (
	"io"
	"strings"

	"github.com/rs/zerolog"
	dialogv1 "github.com/sentiric/sentiric-contracts/gen/go/sentiric/dialog/v1"
	"github.com/sentiric/sentiric-dialog-service/internal/clients/llm"
	"github.com/sentiric/sentiric-dialog-service/internal/state"
	"google.golang.org/grpc/codes"
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

// StreamConversation: Bi-directional streaming RPC
func (s *DialogService) StreamConversation(stream dialogv1.DialogService_StreamConversationServer) error {
	ctx := stream.Context()
	var currentSession *state.Session
	var currentInputBuffer strings.Builder

	s.log.Info().Msg("Yeni konuşma akışı başladı")

	for {
		// 1. İstemciden Mesaj Al
		in, err := stream.Recv()
		if err == io.EOF {
			return nil
		}
		if err != nil {
			s.log.Error().Err(err).Msg("Stream alma hatası")
			return err
		}

		// 2. Payload Tipine Göre İşle
		switch payload := in.Payload.(type) {
		
		// A. Konfigürasyon (Oturum Başlatma)
		case *dialogv1.StreamConversationRequest_Config:
			sessionID := payload.Config.SessionId
			userID := payload.Config.UserId
			
			sess, err := s.stateManager.GetSession(ctx, sessionID)
			if err != nil {
				return status.Errorf(codes.Internal, "Oturum yüklenemedi: %v", err)
			}
			sess.UserID = userID
			currentSession = sess
			s.log.Info().Str("session_id", sessionID).Msg("Oturum yüklendi/oluşturuldu")

		// B. Metin Girdisi (STT'den parça parça gelir)
		case *dialogv1.StreamConversationRequest_TextInput:
			if currentSession == nil {
				return status.Error(codes.FailedPrecondition, "Önce ConversationConfig gönderilmeli")
			}
			currentInputBuffer.WriteString(payload.TextInput)

		// C. Girdi Sonu Sinyali (Kullanıcı sustu)
		case *dialogv1.StreamConversationRequest_IsFinalInput:
			if !payload.IsFinalInput {
				continue
			}
			
			userText := currentInputBuffer.String()
			if userText == "" {
				continue 
			}
			s.log.Info().Str("input", userText).Msg("User input complete, sending to LLM") // İngilizce

			// Geçmişe ekle
			s.stateManager.AddTurn(currentSession, "user", userText)
			currentInputBuffer.Reset()

			// SessionID'yi TraceID olarak kullanıyoruz
			traceID := currentSession.SessionID 

			// LLM Çağrısı
			tokensChan, err := s.llmClient.Generate(ctx, traceID, currentSession.History, userText)
			if err != nil {
                // [FIX] Canceled hatasını ayıkla
                if status.Code(err) == codes.Canceled {
                    s.log.Warn().Msg("LLM stream canceled by client")
                } else {
				    s.log.Error().Err(err).Str("trace_id", traceID).Msg("LLM call failed")
                }
				return err
			}

			// LLM Yanıtını İstemciye Aktar (Token Token)
			var fullResponse strings.Builder
			
			for token := range tokensChan {
				fullResponse.WriteString(token)
				err := stream.Send(&dialogv1.StreamConversationResponse{
					Payload: &dialogv1.StreamConversationResponse_TextResponse{
						TextResponse: token,
					},
				})
				if err != nil {
					return err
				}
			}

			// Yanıt bitti sinyali
			err = stream.Send(&dialogv1.StreamConversationResponse{
				Payload: &dialogv1.StreamConversationResponse_IsFinalResponse{
					IsFinalResponse: true,
				},
			})
			if err != nil {
				return err
			}

			// AI Cevabını Geçmişe Kaydet
			s.stateManager.AddTurn(currentSession, "assistant", fullResponse.String())
			
			// Redis'i Güncelle
			if err := s.stateManager.SaveSession(ctx, currentSession); err != nil {
				s.log.Error().Err(err).Msg("Oturum kaydedilemedi")
			}
		}
	}
}