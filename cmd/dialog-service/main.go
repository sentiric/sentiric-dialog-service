package main

import (
	"context"
	"fmt"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	dialogv1 "github.com/sentiric/sentiric-contracts/gen/go/sentiric/dialog/v1"
	"github.com/sentiric/sentiric-dialog-service/internal/clients/llm"
	"github.com/sentiric/sentiric-dialog-service/internal/config"
	"github.com/sentiric/sentiric-dialog-service/internal/database"
	"github.com/sentiric/sentiric-dialog-service/internal/logger"
	"github.com/sentiric/sentiric-dialog-service/internal/server"
	"github.com/sentiric/sentiric-dialog-service/internal/service"
	"github.com/sentiric/sentiric-dialog-service/internal/state"
)

var (
	ServiceVersion = "1.0.1"
)

func main() {
	// 1. Config Yükle
	cfg, err := config.Load()
	if err != nil {
		fmt.Fprintf(os.Stderr, "Config yüklenemedi: %v\n", err)
		os.Exit(1)
	}
	
	log := logger.New("dialog-service", cfg.Env, cfg.LogLevel)
	log.Info().Str("version", ServiceVersion).Msg("Servis başlatılıyor...")

	// 2. Bağımlılıklar (Redis & LLM)
	redisClient, err := database.NewRedisClient(cfg.RedisURL, log)
	if err != nil {
		log.Fatal().Err(err).Msg("Redis bağlantısı başarısız")
	}
	defer redisClient.Close()

	stateManager := state.NewManager(redisClient.Client, log)

	var llmClient llm.Client
	if cfg.MockLLM {
		log.Info().Msg("🎭 MOCK LLM Modu Aktif")
		llmClient = llm.NewMockClient()
	} else {
		// DÜZELTME: Config'den gelen sertifika yolları buraya eklendi
		llmClient, err = llm.NewGatewayClient(
			cfg.LLMGatewayURL, 
			cfg.CertPath, 
			cfg.KeyPath, 
			cfg.CaPath, 
			log,
		)
		if err != nil {
			log.Fatal().Err(err).Msg("LLM Gateway bağlantı hatası")
		}
	}
	defer llmClient.Close()

	// 3. gRPC Servisi ve Sunucusu
	dialogSvc := service.NewDialogService(stateManager, llmClient, log)
	grpcServer := server.NewGrpcServer(cfg.CertPath, cfg.KeyPath, cfg.CaPath, log)
	dialogv1.RegisterDialogServiceServer(grpcServer, dialogSvc)

	// 4. HTTP Sunucusu (Health Check İçin)
	http.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		w.WriteHeader(http.StatusOK)
		w.Write([]byte("OK"))
	})

	httpServer := &http.Server{
		Addr: fmt.Sprintf(":%s", cfg.HttpPort),
	}

	// 5. Sunucuları Başlat (Async)
	go func() {
		log.Info().Str("port", cfg.GRPCPort).Msg("gRPC sunucusu dinleniyor")
		if err := server.Start(grpcServer, cfg.GRPCPort); err != nil {
			log.Fatal().Err(err).Msg("gRPC sunucusu hatası")
		}
	}()

	go func() {
		log.Info().Str("port", cfg.HttpPort).Msg("HTTP sunucusu dinleniyor")
		if err := httpServer.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatal().Err(err).Msg("HTTP sunucusu hatası")
		}
	}()

	// 6. Graceful Shutdown
	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	<-quit

	log.Warn().Msg("Kapatma sinyali alındı...")

	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	
	if err := httpServer.Shutdown(ctx); err != nil {
		log.Error().Err(err).Msg("HTTP sunucusu kapatılırken hata oluştu")
	}

	server.Stop(grpcServer)
	
	log.Info().Msg("Servis başarıyla kapatıldı.")
}