// sentiric-dialog-service/internal/service/processor.go
package service

import (
	"strings"
)

// SentenceBuffer, LLM'den gelen token'ları biriktirir ve cümle bitişlerinde teslim eder.
type SentenceBuffer struct {
	builder strings.Builder
}

func NewSentenceBuffer() *SentenceBuffer {
	return &SentenceBuffer{}
}

// Push, yeni bir token ekler. Eğer cümle bittiyse (., !, ?, \n) cümleyi döndürür.
func (sb *SentenceBuffer) Push(token string) (string, bool) {
	sb.builder.WriteString(token)
	current := sb.builder.String()

	// Cümle sonu işaretleri kontrolü (Türkçe ve İngilizce uyumlu)
	if len(token) > 0 {
		lastChar := token[len(token)-1:]
		if strings.ContainsAny(lastChar, ".?!:;\n") {
			fullSentence := strings.TrimSpace(current)
			sb.builder.Reset()
			if fullSentence != "" {
				return fullSentence, true
			}
		}
	}

	// Çok uzun metinlerde (noktalama yoksa) 100 karakterde bir zorunlu flush
	if sb.builder.Len() > 100 && strings.HasSuffix(token, " ") {
		fullSentence := strings.TrimSpace(current)
		sb.builder.Reset()
		return fullSentence, true
	}

	return "", false
}

// Flush, buffer'da kalan son metni (noktalama olmasa bile) döndürür.
func (sb *SentenceBuffer) Flush() (string, bool) {
	final := strings.TrimSpace(sb.builder.String())
	sb.builder.Reset()
	if final != "" {
		return final, true
	}
	return "", false
}
