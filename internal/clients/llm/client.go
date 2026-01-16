package llm

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"fmt"
	"io"
	"os"
	"strings"
	"time"

	"github.com/rs/zerolog"
	llmv1 "github.com/sentiric/sentiric-contracts/gen/go/sentiric/llm/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"
)

type Client interface {
	Generate(ctx context.Context, traceID string, history []*llmv1.ConversationTurn, prompt string) (chan string, error)
	Close()
}

// --- Real Implementation ---
type GatewayClient struct {
	conn   *grpc.ClientConn
	client llmv1.LlmGatewayServiceClient
	log    zerolog.Logger
}

func NewGatewayClient(targetURL string, certPath, keyPath, caPath string, log zerolog.Logger) (*GatewayClient, error) {
	var opts []grpc.DialOption

	// [FIX] URL Sanitization: "https://" veya "http://" ön eklerini kaldır.
	// Go gRPC client sadece "host:port" formatını kabul eder.
	cleanTarget := targetURL
	if strings.HasPrefix(cleanTarget, "https://") {
		cleanTarget = strings.TrimPrefix(cleanTarget, "https://")
	} else if strings.HasPrefix(cleanTarget, "http://") {
		cleanTarget = strings.TrimPrefix(cleanTarget, "http://")
	}

	// ServerName (SNI) için portu ayır (örn: "llm-gateway-service:16021" -> "llm-gateway-service")
	serverName := strings.Split(cleanTarget, ":")[0]

	log.Info().
		Str("original_url", targetURL).
		Str("clean_target", cleanTarget).
		Str("sni", serverName).
		Msg("LLM Gateway bağlantısı hazırlanıyor")

	// mTLS Konfigürasyonu
	if certPath != "" && keyPath != "" && caPath != "" {
		// Dosya varlık kontrolü
		if _, err := os.Stat(certPath); os.IsNotExist(err) {
			log.Warn().Str("path", certPath).Msg("Sertifika dosyası bulunamadı, INSECURE moda geçiliyor.")
			opts = append(opts, grpc.WithTransportCredentials(insecure.NewCredentials()))
		} else {
			tlsConfig, err := loadClientTLS(certPath, keyPath, caPath, serverName)
			if err != nil {
				return nil, fmt.Errorf("client TLS yüklenemedi: %w", err)
			}
			opts = append(opts, grpc.WithTransportCredentials(credentials.NewTLS(tlsConfig)))
			log.Info().Str("target", cleanTarget).Msg("🔐 LLM Gateway bağlantısı için mTLS aktif")
		}
	} else {
		opts = append(opts, grpc.WithTransportCredentials(insecure.NewCredentials()))
		log.Warn().Str("target", cleanTarget).Msg("⚠️ LLM Gateway bağlantısı INSECURE (Şifresiz)")
	}

	// [FIX] Temizlenmiş hedefi kullan
	conn, err := grpc.NewClient(cleanTarget, opts...)
	if err != nil {
		return nil, err
	}

	return &GatewayClient{
		conn:   conn,
		client: llmv1.NewLlmGatewayServiceClient(conn),
		log:    log,
	}, nil
}

func (c *GatewayClient) Generate(ctx context.Context, traceID string, history []*llmv1.ConversationTurn, prompt string) (chan string, error) {
	// 1. Trace ID'yi Metadata'ya Ekle (Context Propagation)
	md := metadata.Pairs("x-trace-id", traceID)
	ctx = metadata.NewOutgoingContext(ctx, md)

	req := &llmv1.GenerateDialogStreamRequest{
		ModelSelector: "local",
		TenantId:      "demo", // TODO: Session'dan gelmeli
		LlamaRequest: &llmv1.GenerateStreamRequest{
			UserPrompt: prompt,
			History:    history,
			Params: &llmv1.GenerationParams{
				MaxNewTokens: int32Ptr(256),
				Temperature:  float32Ptr(0.7),
			},
		},
	}

	stream, err := c.client.GenerateDialogStream(ctx, req)
	if err != nil {
		return nil, err
	}

	outChan := make(chan string)

	go func() {
		defer close(outChan)
		for {
			resp, err := stream.Recv()
			if err == io.EOF {
				return
			}
			if err != nil {
				c.log.Error().Err(err).Str("trace_id", traceID).Msg("LLM Stream hatası")
				return
			}
			
			if llamaResp := resp.GetLlamaResponse(); llamaResp != nil {
				if token := llamaResp.GetToken(); len(token) > 0 {
					outChan <- string(token)
				}
			}
		}
	}()

	return outChan, nil
}

func (c *GatewayClient) Close() {
	if c.conn != nil {
		c.conn.Close()
	}
}

// --- Helper: Load Client TLS ---
// serverName parametresi eklendi
func loadClientTLS(certPath, keyPath, caPath, serverName string) (*tls.Config, error) {
	// 1. İstemci Sertifikası (Client Auth için)
	certificate, err := tls.LoadX509KeyPair(certPath, keyPath)
	if err != nil {
		return nil, err
	}

	// 2. CA Sertifikası (Sunucuyu doğrulamak için)
	caCert, err := os.ReadFile(caPath)
	if err != nil {
		return nil, err
	}
	caPool := x509.NewCertPool()
	if !caPool.AppendCertsFromPEM(caCert) {
		return nil, fmt.Errorf("failed to append CA cert")
	}

	return &tls.Config{
		Certificates: []tls.Certificate{certificate},
		RootCAs:      caPool,
		ServerName:   serverName, // DÜZELTME: Dinamik serverName (örn: llm-gateway-service)
	}, nil
}

// --- Mock Implementation ---
type MockClient struct {}

func NewMockClient() *MockClient {
	return &MockClient{}
}

func (m *MockClient) Generate(ctx context.Context, traceID string, history []*llmv1.ConversationTurn, prompt string) (chan string, error) {
	outChan := make(chan string)
	go func() {
		defer close(outChan)
		response := fmt.Sprintf("MOCK [%s]: '%s' dediniz. Ben Sentiric Dialog Service.", traceID, prompt)
		
		for _, char := range response {
			outChan <- string(char)
			time.Sleep(20 * time.Millisecond)
		}
	}()
	return outChan, nil
}

func (m *MockClient) Close() {}

// --- Helper Functions for Protobuf Pointers ---
func int32Ptr(v int32) *int32 {
	return &v
}

func float32Ptr(v float32) *float32 {
	return &v
}