# 🧠 Sentiric Dialog Service

Platformun kanaldan bağımsız (Web, Mobil, Telefon) **konuşma beynidir.**

## 🎯 Sorumluluklar
1.  **Durum Yönetimi (Redis):** Konuşma geçmişini ve o anki adımı tutar.
2.  **LLM Orkestrasyonu:** Kullanıcı girdisini alır, RAG ile zenginleştirir, LLM Gateway'e gönderir.
3.  **Mantık:** LLM'den gelen yanıtı işler (JSON parse vb.) ve bir sonraki aksiyonu belirler (Konuş, Transfer Et, Kapat).

## 🔌 API
- `StreamConversation` (gRPC Bi-directional Stream)

## Test
insecure test
```bash
grpcurl -plaintext -d @ localhost:12061 sentiric.dialog.v1.DialogService/StreamConversation <<EOM
{"config": {"session_id": "test-session-1", "user_id": "tester"}}
{"text_input": "Merhaba Sentiric"}
{"is_final_input": true}
EOM
```