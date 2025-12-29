package config

import (
	"fmt"
	"os"
	"strings"

	"github.com/joho/godotenv"
)

type Config struct {
	GRPCPort string
	HttpPort string
	CertPath string
	KeyPath  string
	CaPath   string
	LogLevel string
	Env      string

	// External Services
	LLMGatewayURL string
	RedisURL      string
	
	// Feature Flags
	MockLLM bool
}

func Load() (*Config, error) {
	godotenv.Load() // .env varsa yükle

	return &Config{
		GRPCPort: GetEnv("DIALOG_SERVICE_GRPC_PORT", "12061"),
		HttpPort: GetEnv("DIALOG_SERVICE_HTTP_PORT", "12060"),
		
		// Güvenlik dosyaları yoksa boş geçebiliriz (Insecure mode için)
		CertPath: GetEnv("DIALOG_SERVICE_CERT_PATH", ""),
		KeyPath:  GetEnv("DIALOG_SERVICE_KEY_PATH", ""),
		CaPath:   GetEnv("GRPC_TLS_CA_PATH", ""),
		
		LogLevel: GetEnv("LOG_LEVEL", "info"),
		Env:      GetEnv("ENV", "development"),

		LLMGatewayURL: GetEnv("LLM_GATEWAY_SERVICE_TARGET", "llm-gateway-service:16021"),
		RedisURL:      GetEnv("REDIS_URL", "redis:6379"),
		
		MockLLM: GetEnv("MOCK_LLM", "false") == "true",
	}, nil
}

func GetEnv(key, fallback string) string {
	if value, exists := os.LookupEnv(key); exists {
		return strings.TrimSpace(value)
	}
	return fallback
}

func GetEnvOrFail(key string) string {
	value, exists := os.LookupEnv(key)
	if !exists {
		// Zerolog yerine standart çıktı kullanıyoruz, bu pakette log bağımlılığına gerek yok.
		fmt.Fprintf(os.Stderr, "FATAL: Gerekli ortam değişkeni tanımlı değil: %s\n", key)
		os.Exit(1)
	}
	return strings.TrimSpace(value)
}