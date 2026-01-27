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

// StreamConversation: Bi-directional streaming RPC
func (s *DialogService) StreamConversation(stream dialogv1.DialogService_StreamConversationServer) error {
	ctx := stream.Context()

	// Trace ID Extraction
	traceID := "unknown"
	if md, ok := metadata.FromIncomingContext(ctx); ok {
		if ids := md.Get("x-trace-id"); len(ids) > 0 {
			traceID = ids[0]
		}
	}

	l := s.log.With().Str("trace_id", traceID).Logger()
	l.Info().Msg("Yeni dialog stream başlatıldı")

	var currentSession *state.Session
	var currentInputBuffer strings.Builder

	for {
		// 1. İstemciden Mesaj Al
		in, err := stream.Recv()
		if err == io.EOF {
			l.Info().Msg("İstemci stream'i kapattı (EOF)")
			return nil
		}
		if err != nil {
			if status.Code(err) == codes.Canceled {
				l.Warn().Msg("Stream istemci tarafından iptal edildi")
				return nil
			}
			l.Error().Err(err).Msg("Stream alma hatası")
			return err
		}

		// 2. Payload Tipine Göre İşle
		switch payload := in.Payload.(type) {

		// A. Konfigürasyon (Oturum Başlatma)
		case *dialogv1.StreamConversationRequest_Config:
			sessionID := payload.Config.SessionId
			userID := payload.Config.UserId

			l = l.With().Str("session_id", sessionID).Str("user_id", userID).Logger()

			sess, err := s.stateManager.GetSession(ctx, sessionID)
			if err != nil {
				l.Error().Err(err).Msg("Oturum yüklenirken Redis hatası")
				return status.Errorf(codes.Internal, "Oturum yüklenemedi")
			}
			sess.UserID = userID
			currentSession = sess
			l.Info().Int("history_len", len(sess.History)).Msg("Oturum bağlamı yüklendi")

			// --- YENİ EKLENEN KISIM: PROAKTİF KARŞILAMA ---
			// Eğer konuşma geçmişi boşsa (yeni arama), AI ilk sözü söylemelidir.
			if len(sess.History) == 0 {
				l.Info().Msg("Yeni oturum: Asistan otomatik karşılama mesajı üretiyor...")

				// LLM'e özel bir "başlangıç" komutu gönderiyoruz.
				greetingPrompt := "Sen bir telefon asistanısın. Kullanıcıyı kibarca karşıla ve nasıl yardımcı olabileceğini sor. Lütfen kısa ve net ol."

				// LLM Çağrısı (Geçmiş + Greeting Prompt)
				// ✅ RETRY ENTEGRASYONU
				tokensChan, err := retry.WithExponentialBackoff(ctx, func(ctx context.Context) (<-chan string, error) {
					return s.llmClient.Generate(ctx, traceID, currentSession.History, greetingPrompt)
				}, 3)
				if err != nil {
					l.Error().Err(err).Msg("LLM karşılama çağrısı başarısız, kullanıcı girdisi bekleniyor.")
					// Hata olsa bile stream kopmasın, kullanıcı konuşabilir.
					continue
				}

				var fullResponse strings.Builder
				for token := range tokensChan {
					fullResponse.WriteString(token)
					err := stream.Send(&dialogv1.StreamConversationResponse{
						Payload: &dialogv1.StreamConversationResponse_TextResponse{
							TextResponse: token,
						},
					})
					if err != nil {
						l.Error().Err(err).Msg("Token istemciye gönderilemedi")
						return err
					}
				}

				// Final sinyali
				stream.Send(&dialogv1.StreamConversationResponse{
					Payload: &dialogv1.StreamConversationResponse_IsFinalResponse{
						IsFinalResponse: true,
					},
				})

				// Asistan cevabını geçmişe kaydet
				aiResponse := fullResponse.String()
				s.stateManager.AddTurn(currentSession, "assistant", aiResponse)

				// Durumu kaydet
				if err := s.stateManager.SaveSession(ctx, currentSession); err != nil {
					l.Error().Err(err).Msg("Oturum durumu kaydedilemedi (Veri kaybı riski!)")
				}

				l.Info().Str("response", aiResponse).Msg("Karşılama mesajı tamamlandı ve gönderildi.")
			}
			// --- YENİ KISIM SONU ---

		// B. Metin Girdisi (STT'den parça parça gelir)
		case *dialogv1.StreamConversationRequest_TextInput:
			if currentSession == nil {
				l.Warn().Msg("Config gelmeden metin verisi alındı")
				return status.Error(codes.FailedPrecondition, "Önce ConversationConfig gönderilmeli")
			}
			currentInputBuffer.WriteString(payload.TextInput)

		// C. Girdi Sonu Sinyali (Kullanıcı sustu, LLM sırası)
		case *dialogv1.StreamConversationRequest_IsFinalInput:
			if !payload.IsFinalInput {
				continue
			}

			userText := strings.TrimSpace(currentInputBuffer.String())
			if userText == "" {
				l.Debug().Msg("Boş girdi alındı, işlem atlanıyor")
				continue
			}

			l.Info().Str("input", userText).Msg("Kullanıcı girdisi tamamlandı, LLM'e gönderiliyor")

			// 1. Kullanıcı girdisini geçmişe ekle (Bellekte)
			s.stateManager.AddTurn(currentSession, "user", userText)
			currentInputBuffer.Reset()

			// 2. LLM Çağrısı (Stream)
			// ✅ RETRY ENTEGRASYONU
			tokensChan, err := retry.WithExponentialBackoff(ctx, func(ctx context.Context) (<-chan string, error) {
				return s.llmClient.Generate(ctx, traceID, currentSession.History, userText)
			}, 3)
			if err != nil {
				l.Error().Err(err).Msg("LLM servisine erişim hatası")
				return status.Errorf(codes.Unavailable, "Yapay zeka servisi şu an yanıt veremiyor")
			}

			// 3. LLM Yanıtını İstemciye Aktar (Token Token)
			var fullResponse strings.Builder

			for token := range tokensChan {
				fullResponse.WriteString(token)
				err := stream.Send(&dialogv1.StreamConversationResponse{
					Payload: &dialogv1.StreamConversationResponse_TextResponse{
						TextResponse: token,
					},
				})
				if err != nil {
					l.Error().Err(err).Msg("Token istemciye gönderilemedi")
					return err
				}
			}

			// 4. Yanıt bitti sinyali
			err = stream.Send(&dialogv1.StreamConversationResponse{
				Payload: &dialogv1.StreamConversationResponse_IsFinalResponse{
					IsFinalResponse: true,
				},
			})
			if err != nil {
				l.Error().Err(err).Msg("Final sinyali gönderilemedi")
				return err
			}

			// 5. Asistan cevabını geçmişe ekle ve Redis'e kaydet
			aiResponse := fullResponse.String()
			s.stateManager.AddTurn(currentSession, "assistant", aiResponse)

			if err := s.stateManager.SaveSession(ctx, currentSession); err != nil {
				l.Error().Err(err).Msg("Oturum durumu kaydedilemedi (Veri kaybı riski!)")
			} else {
				l.Info().Int("response_len", len(aiResponse)).Msg("Tur tamamlandı ve kaydedildi")
			}
		}
	}
}
