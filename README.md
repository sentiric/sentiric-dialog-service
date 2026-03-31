# 🧠 Sentiric Dialog Service (v1.0.1)

[![Status](https://img.shields.io/badge/status-production_ready-green.svg)]()
[![Security](https://img.shields.io/badge/security-mTLS_Strict-blue.svg)]()
[![Architecture](https://img.shields.io/badge/architecture-Nano_Edge_Ready-orange.svg)]()

**Sentiric Dialog Service**, platformun "Konuşma Beyni ve Hafıza Yöneticisi"dir. Kullanıcının niyetini anlar, oturum bazlı konuşma geçmişini yönetir, RAG üzerinden kurumsal bağlamı çeker ve LLM Gateway üzerinden anlık (streaming) yanıt üretir.

## 🚀 Temel Yetenekler
1. **Full-Duplex Streaming:** Metni alır, Token Token sese gidecek veriyi anında yayınlar (0 gecikme).
2. **Nano-Edge State Management:** Redis (L2 Cache) çökerse bile `DashMap` (L1 Cache) kullanarak hafıza kaybı yaşamadan çalışmaya devam eder.
3. **Zero Trust Security:** Diğer servislere bağlanırken ve bağlantı kabul ederken **mTLS (Karşılıklı TLS)** zorunludur.
4. **SUTS v4.0 Observability:** Tüm loglar makine-okunabilir JSON formatındadır ve Context Propagation (`x-trace-id`) kesintisiz işler.

## 🛠️ Kurulum ve Ortam Değişkenleri

```bash
DIALOG_SERVICE_GRPC_PORT=12061
DIALOG_SERVICE_HTTP_PORT=12060
REDIS_URL=redis://redis.service.sentiric.cloud:6379/0

# Bağımlılıklar
LLM_GATEWAY_SERVICE_TARGET=https://llm-gateway-service:16021
KNOWLEDGE_QUERY_SERVICE_GRPC_URL=https://knowledge-query-service:17021

# mTLS (Zorunlu)
GRPC_TLS_CA_PATH=/certs/ca.crt
DIALOG_SERVICE_CERT_PATH=/certs/dialog-service.crt
DIALOG_SERVICE_KEY_PATH=/certs/dialog-service.key
```
---
