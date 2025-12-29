package llm

import (
	"context"
	"fmt"
	"io"
	"time"

	"github.com/rs/zerolog"
	llmv1 "github.com/sentiric/sentiric-contracts/gen/go/sentiric/llm/v1"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"
)

type Client interface {
	Generate(ctx context.Context, history []*llmv1.ConversationTurn, prompt string) (chan string, error)
	Close()
}

// --- Real Implementation ---
type GatewayClient struct {
	conn   *grpc.ClientConn
	client llmv1.LlmGatewayServiceClient
	log    zerolog.Logger
}

func NewGatewayClient(target string, log zerolog.Logger) (*GatewayClient, error) {
	// TODO: Production'da mTLS credentials kullanılmalı
	conn, err := grpc.NewClient(target, grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		return nil, err
	}

	return &GatewayClient{
		conn:   conn,
		client: llmv1.NewLlmGatewayServiceClient(conn),
		log:    log,
	}, nil
}

func (c *GatewayClient) Generate(ctx context.Context, history []*llmv1.ConversationTurn, prompt string) (chan string, error) {
	req := &llmv1.GenerateDialogStreamRequest{
		ModelSelector: "local",
		TenantId:      "demo", // TODO: Session'dan gelmeli
		LlamaRequest: &llmv1.GenerateStreamRequest{
			UserPrompt: prompt,
			History:    history,
			Params: &llmv1.GenerationParams{
				// DÜZELTME: Pointer helper fonksiyonları kullanılıyor
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
				c.log.Error().Err(err).Msg("LLM Stream hatası")
				return
			}
			
			if llamaResp := resp.GetLlamaResponse(); llamaResp != nil {
				// Bytes to String conversion
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

// --- Mock Implementation ---
type MockClient struct {}

func NewMockClient() *MockClient {
	return &MockClient{}
}

func (m *MockClient) Generate(ctx context.Context, history []*llmv1.ConversationTurn, prompt string) (chan string, error) {
	outChan := make(chan string)
	go func() {
		defer close(outChan)
		response := fmt.Sprintf("MOCK: '%s' dediniz. Ben Sentiric Dialog Service.", prompt)
		
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