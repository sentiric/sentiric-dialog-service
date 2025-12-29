# 🧠 Sentiric Dialog Service

[![Status](https://img.shields.io/badge/status-production_ready-green.svg)]()
[![Architecture](https://img.shields.io/badge/architecture-stateful_grpc-blue.svg)]()

Sentiric platformunun **"Konuşma Beyni"**. Kullanıcının niyetini anlar, konuşma geçmişini yönetir ve LLM (Llama) ile mantıklı yanıtlar üretir.

## 🎯 Sorumluluklar

1.  **State Management (Redis):** Her oturum (`session_id`) için konuşma geçmişini ve değişkenleri saklar.
2.  **LLM Orchestration:** `llm-gateway` üzerinden Llama modeline bağlanır.
3.  **Streaming:** Kullanıcıdan gelen parçalı metni (STT) alıp, LLM'den gelen parçalı yanıtı (TTS) anlık iletir.
4.  **Security:** Tüm dış bağlantılarda **mTLS** ve **Trace ID Propagation** kullanır.

## 🚀 Hızlı Başlangıç

### Ön Gereksinimler
*   Docker & Docker Compose
*   `sentiric-certificates` (Bir üst dizinde olmalı)

### 1. Geliştirme Modu (Hızlı & Mock)
GPU gerektirmez. LLM yerine "Echo" yanıtı döner.
```bash
docker compose -f docker-compose.dev.yml up --build
```

### 2. Entegrasyon Modu (Gerçek Zeka & GPU)
Gerçek Llama modeli ve Gateway ile çalışır. (Nvidia GPU gerekir).
```bash
docker compose -f docker-compose.integration.yml up --build
```

## 🛠️ Konfigürasyon

| Değişken | Varsayılan | Açıklama |
|---|---|---|
| `DIALOG_SERVICE_GRPC_PORT` | `12061` | Servis portu |
| `REDIS_URL` | `redis:6379` | Durum sunucusu |
| `MOCK_LLM` | `false` | `true` ise LLM'e gitmez, fake cevap döner |
| `LLM_GATEWAY_SERVICE_TARGET` | `...:16021` | Hedef Gateway adresi |
| `GRPC_TLS_CA_PATH` | `/certs/ca.crt` | mTLS Kök Sertifikası |

## 🧪 Test
Detaylı test komutları için [TEST.md](TEST.md) dosyasına bakınız.