// sentiric-dialog-service/internal/service/processor.go
package service

import (
	"strings"
)

// SentenceBuffer, LLM'den gelen küçük token parçalarını anlamlı cümlelere birleştirir.
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

	if len(token) > 0 {
		// Son karakteri kontrol et
		lastChar := token[len(token)-1:]
		if strings.ContainsAny(lastChar, ".?!:;\n") {
			fullSentence := strings.TrimSpace(current)
			sb.builder.Reset()
			if fullSentence != "" {
				return fullSentence, true
			}
		}
	}

	// Emniyet kilidi: Cümle 150 karakteri geçerse zorunlu flush (TTS gecikmesini önlemek için)
	if sb.builder.Len() > 150 && strings.HasSuffix(token, " ") {
		fullSentence := strings.TrimSpace(current)
		sb.builder.Reset()
		return fullSentence, true
	}

	return "", false
}

func (sb *SentenceBuffer) Flush() (string, bool) {
	final := strings.TrimSpace(sb.builder.String())
	sb.builder.Reset()
	if final != "" {
		return final, true
	}
	return "", false
}
