package database

import (
	"context"
	"time"

	"github.com/redis/go-redis/v9"
	"github.com/rs/zerolog"
)

type RedisClient struct {
	Client *redis.Client
	Log    zerolog.Logger
}

func NewRedisClient(addr string, log zerolog.Logger) (*RedisClient, error) {
	rdb := redis.NewClient(&redis.Options{
		Addr:     addr,
		Password: "", // Şimdilik şifresiz, prod için env'den alınmalı
		DB:       0,
	})

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	if err := rdb.Ping(ctx).Err(); err != nil {
		log.Error().Err(err).Msg("Redis bağlantısı başarısız")
		return nil, err
	}

	log.Info().Str("addr", addr).Msg("Redis bağlantısı başarılı")
	return &RedisClient{Client: rdb, Log: log}, nil
}

func (r *RedisClient) Close() {
	if r.Client != nil {
		r.Client.Close()
	}
}