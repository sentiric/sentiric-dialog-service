## Test
insecure test
```bash
grpcurl -plaintext -d @ localhost:12061 sentiric.dialog.v1.DialogService/StreamConversation <<EOM
{"config": {"session_id": "test-session-1", "user_id": "tester"}}
{"text_input": "Merhaba Sentiric"}
{"is_final_input": true}
EOM
```

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
{"config": {"session_id": "real-integration-test-3", "user_id": "admin"}}
{"text_input": "Merhaba, kendini tanıtır mısın?"}
{"is_final_input": true}
EOM
```