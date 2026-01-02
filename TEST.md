# 🧪 Test Prosedürleri

Servisi test etmek için `grpcurl` ve Docker kullanıyoruz.

## 1. Mock Testi (Geliştirici Modu)
`docker-compose.yml` çalışırken:

```bash
docker run --rm -i --network host fullstorydev/grpcurl -plaintext -d @ localhost:12061 sentiric.dialog.v1.DialogService/StreamConversation <<EOM
{"config": {"session_id": "mock-test", "user_id": "dev"}}
{"text_input": "Merhaba"}
{"is_final_input": true}
EOM
```
*Beklenen:* "MOCK: 'Merhaba' dediniz..."

## 2. Gerçek Zeka Testi (Entegrasyon Modu)
`docker-compose.yml` çalışırken (Sertifikalarla):

```bash
docker run --rm -i \
  --network host \
  -v $(pwd)/../sentiric-certificates/certs:/certs \
  fullstorydev/grpcurl \
  -d @ \
  -cacert /certs/ca.crt \
  -cert /certs/dialog-service-chain.crt \
  -key /certs/dialog-service.key \
  localhost:12061 sentiric.dialog.v1.DialogService/StreamConversation <<EOM
{"config": {"session_id": "real-test-1", "user_id": "admin"}}
{"text_input": "Merhaba, nasılsın?"}
{"is_final_input": true}
EOM
```
*Beklenen:* Llama modelinden gelen anlamlı Türkçe cevap.