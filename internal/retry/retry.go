// sentiric-dialog-service/internal/retry/retry.go
// ✅ YENİ: Exponential Backoff Retry Helper

package retry

import (
	"context"
	"fmt"
	"time"

	"github.com/rs/zerolog/log"
)

// WithExponentialBackoff retries a function with exponential backoff
// Backoff: 1s, 2s, 4s
func WithExponentialBackoff[T any](
	ctx context.Context,
	fn func(ctx context.Context) (T, error),
	maxRetries int,
) (T, error) {
	var result T
	var err error

	for attempt := 0; attempt < maxRetries; attempt++ {
		result, err = fn(ctx)
		if err == nil {
			if attempt > 0 {
				log.Info().
					Int("attempt", attempt+1).
					Msg("✅ Retry başarılı")
			}
			return result, nil
		}

		if attempt < maxRetries-1 {
			backoff := time.Duration(1<<uint(attempt)) * time.Second // 1s, 2s, 4s
			log.Warn().
				Err(err).
				Int("attempt", attempt+1).
				Int("max_retries", maxRetries).
				Dur("backoff", backoff).
				Msg("⚠️ Servis çağrısı başarısız, retry ediliyor...")

			select {
			case <-time.After(backoff):
				// Continue to next attempt
			case <-ctx.Done():
				return result, ctx.Err()
			}
		}
	}

	return result, fmt.Errorf("max retry (%d) aşıldı: %w", maxRetries, err)
}
