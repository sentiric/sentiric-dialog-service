## Test
insecure test
```bash
grpcurl -plaintext -d @ localhost:12061 sentiric.dialog.v1.DialogService/StreamConversation <<EOM
{"config": {"session_id": "test-session-1", "user_id": "tester"}}
{"text_input": "Merhaba Sentiric"}
{"is_final_input": true}
EOM
```

# 1. Klasörde olduğunuzdan emin olun
cd ~/sentiric/sentiric-dialog-service

# 2. Yeniden derle ve başlat (Entegrasyon dosyası ile)
docker compose -f docker-compose.integration.yml up -d --build


secure integratio test
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
{"config": {"session_id": "TITAN-TEST-001", "user_id": "architect"}}
{"text_input": "Merhaba, sistem mimarisi hakkında ne düşünüyorsun?"}
{"is_final_input": true}
EOM
```