# 🧠 Mantık Mimarisi

## 1. Akış Diyagramı (Streaming Loop)

`StreamConversation` RPC metodu şu döngüyü işletir:

```mermaid
sequenceDiagram
    participant User as Client (Telephony/Web)
    participant Dialog as Dialog Service
    participant Redis as State Store
    participant LLM as LLM Gateway

    User->>Dialog: Config {session_id: "123"}
    Dialog->>Redis: GET session:123
    Redis-->>Dialog: {history: [...]}
    
    loop Streaming Audio/Text
        User->>Dialog: "Mer" -> "Merha" -> "Merhaba"
        Note over Dialog: Bufferlama
        User->>Dialog: IsFinalInput: true
    end

    Dialog->>Dialog: History += "Merhaba"
    Dialog->>LLM: GenerateStream(history, prompt="Merhaba") (mTLS + TraceID)
    
    loop Token Streaming
        LLM-->>Dialog: "Se"
        Dialog-->>User: "Se"
        LLM-->>Dialog: "lam"
        Dialog-->>User: "lam"
    end

    Dialog->>Dialog: History += "Selam"
    Dialog->>Redis: SET session:123 (Updated History)
```

## 2. Güvenlik ve Gözlemlenebilirlik

*   **mTLS:** `internal/clients/llm/client.go` içinde Client Certificate yüklenir. Gateway'e bağlanırken bu sertifika sunulur.
*   **Trace ID:** İstek ile gelen `session_id`, `x-trace-id` header'ı olarak LLM Gateway'e ve oradan Llama Service'e kadar taşınır. Bu sayede loglarda `[TraceID: xyz]` takibi yapılabilir.