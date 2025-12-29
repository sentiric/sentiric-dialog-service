package server

import (
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"net"
	"os"

	"github.com/rs/zerolog"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
)

func NewGrpcServer(certPath, keyPath, caPath string, log zerolog.Logger) *grpc.Server {
	opts := []grpc.ServerOption{}

	// mTLS Kontrolü: Dosyalar varsa secure başlat, yoksa insecure
	if certPath != "" && keyPath != "" {
		creds, err := loadServerTLS(certPath, keyPath, caPath)
		if err != nil {
			log.Warn().Err(err).Msg("TLS yüklenemedi, INSECURE moda geçiliyor")
		} else {
			opts = append(opts, grpc.Creds(creds))
			log.Info().Msg("🔐 mTLS Aktif")
		}
	} else {
		log.Warn().Msg("⚠️ TLS yolları boş, INSECURE modda başlatılıyor")
	}

	return grpc.NewServer(opts...)
}

func Start(grpcServer *grpc.Server, port string) error {
	listener, err := net.Listen("tcp", fmt.Sprintf(":%s", port))
	if err != nil {
		return fmt.Errorf("gRPC portu dinlenemedi: %w", err)
	}
	return grpcServer.Serve(listener)
}

func Stop(grpcServer *grpc.Server) {
	grpcServer.GracefulStop()
}

func loadServerTLS(certPath, keyPath, caPath string) (credentials.TransportCredentials, error) {
	certificate, err := tls.LoadX509KeyPair(certPath, keyPath)
	if err != nil {
		return nil, err
	}

	config := &tls.Config{
		Certificates: []tls.Certificate{certificate},
		ClientAuth:   tls.NoClientCert, // İsteğe bağlı (mTLS zorunluysa RequireAndVerifyClientCert)
	}

	if caPath != "" {
		caCert, err := os.ReadFile(caPath)
		if err == nil {
			caPool := x509.NewCertPool()
			if caPool.AppendCertsFromPEM(caCert) {
				config.ClientCAs = caPool
				config.ClientAuth = tls.RequireAndVerifyClientCert
			}
		}
	}

	return credentials.NewTLS(config), nil
}