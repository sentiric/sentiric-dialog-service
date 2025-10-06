// sentiric-dialog-service/internal/config/config.go
package config

import (
	"os"

	"github.com/joho/godotenv"
	"github.com/rs/zerolog/log"
)

type Config struct {
	GRPCPort string
	HttpPort string
	CertPath string
	KeyPath  string
	CaPath   string
	LogLevel string
	Env      string

	// Diyalog servisi bağımlılıkları (Placeholder)
	AgentServiceURL string
	RedisURL        string
}

func Load() (*Config, error) {
	godotenv.Load()

	return &Config{
		GRPCPort: GetEnv("DIALOG_SERVICE_GRPC_PORT", "12061"),
		HttpPort: GetEnv("DIALOG_SERVICE_HTTP_PORT", "12060"),
		CertPath: GetEnvOrFail("DIALOG_SERVICE_CERT_PATH"),
		KeyPath:  GetEnvOrFail("DIALOG_SERVICE_KEY_PATH"),
		CaPath:   GetEnvOrFail("GRPC_TLS_CA_PATH"),
		LogLevel: GetEnv("LOG_LEVEL", "info"),
		Env:      GetEnv("ENV", "production"),

		AgentServiceURL: GetEnv("AGENT_SERVICE_TARGET_GRPC_URL", "agent-service:12031"),
		RedisURL:        GetEnv("REDIS_URL", "redis:6379"),
	}, nil
}

func GetEnv(key, fallback string) string {
	if value, exists := os.LookupEnv(key); exists {
		return value
	}
	return fallback
}

func GetEnvOrFail(key string) string {
	value, exists := os.LookupEnv(key)
	if !exists {
		log.Fatal().Str("variable", key).Msg("Gerekli ortam değişkeni tanımlı değil")
	}
	return value
}
