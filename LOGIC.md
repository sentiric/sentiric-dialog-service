# 🧬 Dialog Service Hybrid Memory & Orchestration Logic

Bu belge, `dialog-service`'in AI hafızasını nasıl yönettiğini ve Nano-Edge (IoT) ortamlarında Redis olmadan nasıl hayatta kaldığını açıklar.

## 1. Nano-Edge Survival (L1 / L2 Hybrid Cache)
Sistem, her aramanın geçmişini (User: Merhaba, AI: Hoşgeldiniz) bilmek zorundadır.
* **L2 Cache (Redis):** Varsayılan olarak tüm konuşma geçmişi Redis'e `SETEX dialog:session:{id}` olarak 1 saat TTL ile yazılır. 
* **L1 Cache (DashMap - Ghost Mode):** Eğer Redis çökerse veya sistem Nano-Edge (İnternetsiz lokal cihaz) modunda başlatılırsa, servis `panic!` atmaz. Tüm hafıza Rust'ın `DashMap` (Thread-Safe HashMap) yapısına kaydedilir.
* **Auto-Healing Drift Sync:** Redis tekrar çevrimiçi (Online) olursa, L1'deki tüm konuşma verisi asenkron bir task ile Redis'e aktarılır (Flush). Bu işlem sırasında `DashMap` kilitlenmesini (Lock Contention) önlemek için verinin önce hızlıca kopyası (Snapshot) alınır.

## 2. LLM Streaming Orchestration
Kullanıcı konuştuğunda, metin Dialog servisine gelir.
1. Dialog, `knowledge-query` servisine giderek (RAG) bağlamı (Context) çeker.
2. Sistemi ve RAG'ı birleştirip `llm-gateway`'e `GenerateDialogStream` gRPC isteği atar.
3. LLM'den gelen her bir kelime (Token) beklemeden doğrudan çağrıyı yapan sisteme (TAS veya Stream SDK) iletilir (Sıfır gecikme).
4. **IsFinalInput Olayı:** Cümle bittiğinde ve LLM yanıtı tamamen üretildiğinde, tüm diyalog geçmişe (History) kaydedilir ve Crystalline servisi için `dialog.turn.completed` olayı RabbitMQ'ya fırlatılır.
