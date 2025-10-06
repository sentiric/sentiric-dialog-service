// sentiric-dialog-service/cmd/dialog-service/main.go
package main

import (
	"context"
	"fmt"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/rs/zerolog"
	"github.com/sentiric/sentiric-dialog-service/internal/config"
	"github.com/sentiric/sentiric-dialog-service/internal/logger"
	"github.com/sentiric/sentiric-dialog-service/internal/server"

	dialogv1 "github.com/sentiric/sentiric-contracts/gen/go/sentiric/dialog/v1"
)

var (
	ServiceVersion string
	GitCommit      string
	BuildDate      string
)

const serviceName = "dialog-service"

func main() {
	cfg, err := config.Load()
	if err != nil {
		fmt.Fprintf(os.Stderr, "Kritik Hata: Konfigürasyon yüklenemedi: %v\n", err)
		os.Exit(1)
	}

	log := logger.New(serviceName, cfg.Env, cfg.LogLevel)

	log.Info().
		Str("version", ServiceVersion).
		Str("commit", GitCommit).
		Str("build_date", BuildDate).
		Str("profile", cfg.Env).
		Msg("🚀 Sentiric Dialog Service başlatılıyor...")

	// HTTP ve gRPC sunucularını oluştur
	grpcServer := server.NewGrpcServer(cfg.CertPath, cfg.KeyPath, cfg.CaPath, log)
	httpServer := startHttpServer(cfg.HttpPort, log)

	// gRPC Handler'ı kaydet
	dialogv1.RegisterDialogServiceServer(grpcServer, &dialogHandler{})

	// gRPC sunucusunu bir goroutine'de başlat
	go func() {
		log.Info().Str("port", cfg.GRPCPort).Msg("gRPC sunucusu dinleniyor...")
		if err := server.Start(grpcServer, cfg.GRPCPort); err != nil && err.Error() != "http: Server closed" {
			log.Error().Err(err).Msg("gRPC sunucusu başlatılamadı")
		}
	}()

	// Graceful shutdown için sinyal dinleyicisi
	quit := make(chan os.Signal, 1)
	signal.Notify(quit, syscall.SIGINT, syscall.SIGTERM)
	<-quit

	log.Warn().Msg("Kapatma sinyali alındı, servisler durduruluyor...")

	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()

	server.Stop(grpcServer)
	log.Info().Msg("gRPC sunucusu durduruldu.")

	if err := httpServer.Shutdown(ctx); err != nil {
		log.Error().Err(err).Msg("HTTP sunucusu düzgün kapatılamadı.")
	} else {
		log.Info().Msg("HTTP sunucusu durduruldu.")
	}

	log.Info().Msg("Servis başarıyla durduruldu.")
}

func startHttpServer(port string, log zerolog.Logger) *http.Server {
	mux := http.NewServeMux()
	mux.HandleFunc("/health", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "application/json")
		w.WriteHeader(http.StatusOK)
		fmt.Fprintf(w, `{"status": "ok"}`)
	})

	addr := fmt.Sprintf(":%s", port)
	srv := &http.Server{Addr: addr, Handler: mux}

	go func() {
		log.Info().Str("port", port).Msg("HTTP sunucusu (health) dinleniyor")
		if err := srv.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatal().Err(err).Msg("HTTP sunucusu başlatılamadı")
		}
	}()
	return srv
}

// =================================================================
// GRPC HANDLER IMPLEMENTASYONU (Placeholder)
// =================================================================

type dialogHandler struct {
	dialogv1.UnimplementedDialogServiceServer
}

func (*dialogHandler) StartDialog(ctx context.Context, req *dialogv1.StartDialogRequest) (*dialogv1.StartDialogResponse, error) {
	log := zerolog.Ctx(ctx).With().Str("rpc", "StartDialog").Logger()
	log.Info().Msg("StartDialog isteği alındı (Placeholder)")

	return &dialogv1.StartDialogResponse{
		ResponseText:   "Dialog başlatıldı. İlk yanıtınız bekleniyor.",
		NextAction:     "WAIT_FOR_INPUT",
		UpdatedContext: req.Context,
	}, nil
}

func (*dialogHandler) ProcessUserInput(ctx context.Context, req *dialogv1.ProcessUserInputRequest) (*dialogv1.ProcessUserInputResponse, error) {
	log := zerolog.Ctx(ctx).With().Str("rpc", "ProcessUserInput").Str("call_id", req.GetCallId()).Logger()
	log.Info().Str("input", req.GetText()).Msg("ProcessUserInput isteği alındı (Placeholder)")

	if req.GetText() == "exit" {
		return &dialogv1.ProcessUserInputResponse{
			ResponseText: "Güle güle.",
			NextAction:   "TERMINATE_CALL",
		}, nil
	}

	return &dialogv1.ProcessUserInputResponse{
		ResponseText: fmt.Sprintf("Anladım: %s. Ama şu an sadece yer tutucuyum.", req.GetText()),
		NextAction:   "WAIT_FOR_INPUT",
	}, nil
}
