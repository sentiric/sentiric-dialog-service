// sentiric-dialog-service/internal/clients/llm/client.go
package llm

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"io"
	"os"
	"strings"

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

type GatewayClient struct {
	conn   *grpc.ClientConn
	client llmv1.LlmGatewayServiceClient
	log    zerolog.Logger
}

func NewGatewayClient(targetURL string, certPath, keyPath, caPath string, log zerolog.Logger) (*GatewayClient, error) {
	var opts []grpc.DialOption

	// [FIX] URL Sanitization (Smart SNI)
	cleanTarget := targetURL
	for _, prefix := range []string{"https://", "http://"} {
		cleanTarget = strings.TrimPrefix(cleanTarget, prefix)
	}
	serverName := strings.Split(cleanTarget, ":")[0]

	if certPath != "" && keyPath != "" && caPath != "" {
		tlsConfig, err := loadClientTLS(certPath, keyPath, caPath, serverName)
		if err != nil {
			log.Warn().Err(err).Msg("mTLS Load failed, using insecure")
			opts = append(opts, grpc.WithTransportCredentials(insecure.NewCredentials()))
		} else {
			opts = append(opts, grpc.WithTransportCredentials(credentials.NewTLS(tlsConfig)))
			opts = append(opts, grpc.WithAuthority(serverName))
			log.Info().Str("sni", serverName).Msg("🔐 mTLS Connection Secured for LLM")
		}
	} else {
		opts = append(opts, grpc.WithTransportCredentials(insecure.NewCredentials()))
	}

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
	md := metadata.Pairs("x-trace-id", traceID)
	outCtx := metadata.NewOutgoingContext(ctx, md)

	req := &llmv1.GenerateDialogStreamRequest{
		ModelSelector: "local",
		TenantId:      "system",
		LlamaRequest: &llmv1.GenerateStreamRequest{
			UserPrompt: prompt,
			History:    history,
			Params: &llmv1.GenerationParams{
				MaxNewTokens: int32Ptr(512),
				Temperature:  float32Ptr(0.7),
			},
		},
	}

	stream, err := c.client.GenerateDialogStream(outCtx, req)
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
				c.log.Error().Err(err).Str("trace_id", traceID).Msg("LLM Stream error")
				return
			}

			if llamaResp := resp.GetLlamaResponse(); llamaResp != nil {
				// [v1.15.0 FIX]: Token artık bytes tipinde.
				// Doğrudan string'e çeviriyoruz çünkü Go'nun string(bytes) işlemi
				// geçersiz karakterleri '' ile değiştirerek çökmemeyi sağlar.
				if tokenBytes := llamaResp.GetToken(); len(tokenBytes) > 0 {
					outChan <- string(tokenBytes)
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

func loadClientTLS(certPath, keyPath, caPath, serverName string) (*tls.Config, error) {
	certificate, err := tls.LoadX509KeyPair(certPath, keyPath)
	if err != nil {
		return nil, err
	}

	caCert, err := os.ReadFile(caPath)
	if err != nil {
		return nil, err
	}
	caPool := x509.NewCertPool()
	caPool.AppendCertsFromPEM(caCert)

	return &tls.Config{
		Certificates: []tls.Certificate{certificate},
		RootCAs:      caPool,
		ServerName:   serverName,
	}, nil
}

// --- Helpers ---
func int32Ptr(v int32) *int32       { return &v }
func float32Ptr(v float32) *float32 { return &v }
func NewMockClient() *MockClient    { return &MockClient{} }

type MockClient struct{}

func (m *MockClient) Generate(ctx context.Context, tid string, h []*llmv1.ConversationTurn, p string) (chan string, error) {
	c := make(chan string)
	go func() { defer close(c); c <- "MOCK RESPONSE" }()
	return c, nil
}
func (m *MockClient) Close() {}
