# 🧠 Sentiric Dialog Service

Platformun kanaldan bağımsız (Web, Mobil, Telefon) **konuşma beynidir.**

## 🎯 Sorumluluklar
1.  **Durum Yönetimi (Redis):** Konuşma geçmişini ve o anki adımı tutar.
2.  **LLM Orkestrasyonu:** Kullanıcı girdisini alır, RAG ile zenginleştirir, LLM Gateway'e gönderir.
3.  **Mantık:** LLM'den gelen yanıtı işler (JSON parse vb.) ve bir sonraki aksiyonu belirler (Konuş, Transfer Et, Kapat).

## 🔌 API
- `StreamConversation` (gRPC Bi-directional Stream)