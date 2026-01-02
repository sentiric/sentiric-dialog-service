package state

import (
	"context"
	"encoding/json"
	"fmt"
	"time"

	"github.com/redis/go-redis/v9"
	"github.com/rs/zerolog"
	llmv1 "github.com/sentiric/sentiric-contracts/gen/go/sentiric/llm/v1"
)

// SessionTTL: Oturum süresi 1 saat olarak belirlendi. Her işlemde yenilenir.
const SessionTTL = 1 * time.Hour

type Session struct {
	SessionID string                    `json:"sessionId"`
	UserID    string                    `json:"userId"`
	History   []*llmv1.ConversationTurn `json:"history"`
	// Gelecekteki genişlemeler için metadata alanı (örn: kullanıcının dili, tercihleri)
	Metadata  map[string]string         `json:"metadata,omitempty"` 
}

type Manager struct {
	redis *redis.Client
	log   zerolog.Logger
}

func NewManager(redisClient *redis.Client, log zerolog.Logger) *Manager {
	return &Manager{
		redis: redisClient,
		log:   log,
	}
}

func (m *Manager) GetSession(ctx context.Context, sessionID string) (*Session, error) {
	key := fmt.Sprintf("session:%s", sessionID)
	val, err := m.redis.Get(ctx, key).Result()
	
	if err == redis.Nil {
		// Yeni oturum başlat
		m.log.Debug().Str("session_id", sessionID).Msg("Redis'te oturum bulunamadı, yeni oluşturuluyor.")
		return &Session{
			SessionID: sessionID,
			History:   make([]*llmv1.ConversationTurn, 0),
			Metadata:  make(map[string]string),
		}, nil
	} else if err != nil {
		m.log.Error().Err(err).Str("session_id", sessionID).Msg("Redis okuma hatası")
		return nil, err
	}

	var session Session
	if err := json.Unmarshal([]byte(val), &session); err != nil {
		m.log.Error().Err(err).Str("session_id", sessionID).Msg("Session JSON parse hatası")
		return nil, err
	}
	return &session, nil
}

func (m *Manager) SaveSession(ctx context.Context, session *Session) error {
	key := fmt.Sprintf("session:%s", session.SessionID)
	data, err := json.Marshal(session)
	if err != nil {
		return err
	}
	
	// TTL'i her kayıtta yeniliyoruz
	if err := m.redis.Set(ctx, key, data, SessionTTL).Err(); err != nil {
		m.log.Error().Err(err).Str("session_id", session.SessionID).Msg("Redis yazma hatası")
		return err
	}
	
	m.log.Debug().Str("session_id", session.SessionID).Int("turns", len(session.History)).Msg("Oturum durumu kaydedildi.")
	return nil
}

func (m *Manager) AddTurn(session *Session, role string, content string) {
	session.History = append(session.History, &llmv1.ConversationTurn{
		Role:    role,
		Content: content,
	})
}