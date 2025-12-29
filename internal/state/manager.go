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

// SessionTTL: 1 saat işlem yapılmazsa oturum silinir
const SessionTTL = 1 * time.Hour

type Session struct {
	SessionID string                    `json:"sessionId"`
	UserID    string                    `json:"userId"`
	History   []*llmv1.ConversationTurn `json:"history"`
	Variables map[string]string         `json:"variables"`
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
		// Yeni oturum
		return &Session{
			SessionID: sessionID,
			History:   make([]*llmv1.ConversationTurn, 0),
			Variables: make(map[string]string),
		}, nil
	} else if err != nil {
		return nil, err
	}

	var session Session
	if err := json.Unmarshal([]byte(val), &session); err != nil {
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
	return m.redis.Set(ctx, key, data, SessionTTL).Err()
}

func (m *Manager) AddTurn(session *Session, role string, content string) {
	session.History = append(session.History, &llmv1.ConversationTurn{
		Role:    role,
		Content: content,
	})
}