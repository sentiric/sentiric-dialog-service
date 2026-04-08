use crate::clients::knowledge_client::KnowledgeClient;
use crate::clients::llm_client::LlmClient;
use crate::state::manager::StateManager;
// [EKLENDİ]
use crate::state::publisher::GhostPublisher;
use serde_json::json;

use sentiric_contracts::sentiric::dialog::v1::dialog_service_server::DialogService;
use sentiric_contracts::sentiric::dialog::v1::stream_conversation_request::Payload as ReqPayload;
use sentiric_contracts::sentiric::dialog::v1::stream_conversation_response::Payload as RespPayload;
use sentiric_contracts::sentiric::dialog::v1::{
    ProcessUserInputRequest, ProcessUserInputResponse, StartDialogRequest, StartDialogResponse,
    StreamConversationRequest, StreamConversationResponse,
};

use sentiric_contracts::sentiric::llm::v1::generate_stream_response::Type as LlmResponseType;
use sentiric_contracts::sentiric::llm::v1::{
    ConversationTurn, GenerateDialogStreamRequest, GenerateStreamRequest,
};

use futures::StreamExt;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};
use tracing::{error, info};

pub struct DialogServerImpl {
    state_manager: Arc<StateManager>,
    llm_client: Arc<LlmClient>,
    knowledge_client: Arc<KnowledgeClient>,
    publisher: Arc<GhostPublisher>, // [EKLENDİ]
}

impl DialogServerImpl {
    pub fn new(
        state_manager: Arc<StateManager>,
        llm_client: Arc<LlmClient>,
        knowledge_client: Arc<KnowledgeClient>,
        publisher: Arc<GhostPublisher>, // [EKLENDİ]
    ) -> Self {
        Self {
            state_manager,
            llm_client,
            knowledge_client,
            publisher,
        }
    }
}

#[tonic::async_trait]
impl DialogService for DialogServerImpl {
    type StreamConversationStream = ReceiverStream<Result<StreamConversationResponse, Status>>;

    async fn start_dialog(
        &self,
        _request: Request<StartDialogRequest>,
    ) -> Result<Response<StartDialogResponse>, Status> {
        Err(Status::unimplemented("Legacy start_dialog is deprecated. Use stream_conversation for Nano-Edge Architecture."))
    }

    async fn process_user_input(
        &self,
        _request: Request<ProcessUserInputRequest>,
    ) -> Result<Response<ProcessUserInputResponse>, Status> {
        Err(Status::unimplemented("Legacy process_user_input is deprecated. Use stream_conversation for Nano-Edge Architecture."))
    }

    async fn stream_conversation(
        &self,
        request: Request<Streaming<StreamConversationRequest>>,
    ) -> Result<Response<Self::StreamConversationStream>, Status> {
        let trace_id = request
            .metadata()
            .get("x-trace-id")
            .and_then(|m| m.to_str().ok())
            .unwrap_or("unknown_trace")
            .to_string();
        let tenant_id = request
            .metadata()
            .get("x-tenant-id")
            .and_then(|m| m.to_str().ok())
            .unwrap_or("unknown_tenant")
            .to_string();

        info!(event = "DIALOG_TURN_START", trace_id = %trace_id, tenant_id = %tenant_id, "Conversation stream established.");

        let mut stream = request.into_inner();
        let (tx, rx) = tokio::sync::mpsc::channel(128);

        let state_mgr = self.state_manager.clone();
        let llm_cli = self.llm_client.clone();
        let rag_cli = self.knowledge_client.clone();
        let publisher = self.publisher.clone(); // [EKLENDİ]

        tokio::spawn(async move {
            let mut session_id = String::new();
            let mut system_prompt_id = String::new();
            let mut accumulated_text = String::new();

            while let Some(Ok(req)) = stream.next().await {
                match req.payload {
                    Some(ReqPayload::Config(config)) => {
                        session_id = config.session_id.clone();
                        system_prompt_id = config.system_prompt_id.clone();
                    }
                    Some(ReqPayload::TextInput(text)) => {
                        accumulated_text.push_str(&text);
                        accumulated_text.push(' ');
                    }
                    Some(ReqPayload::IsFinalInput(true)) => {
                        let span_id = uuid::Uuid::new_v4().to_string();
                        let user_input = accumulated_text.trim().to_string();

                        let mut history = state_mgr.get_history(&session_id).await;

                        // [ARCH-COMPLIANCE FIX]: Task-05 - RAG Noise/Hallucination Filter
                        let is_filler = user_input.eq_ignore_ascii_case("evet")
                            || user_input.eq_ignore_ascii_case("hayır")
                            || user_input.eq_ignore_ascii_case("tamam")
                            || user_input.len() < 10;

                        let rag_context = if !user_input.is_empty() && !is_filler {
                            if let Some(resp) = rag_cli
                                .query(&tenant_id, &user_input, &trace_id, &span_id)
                                .await
                            {
                                let context_str = resp
                                    .results
                                    .iter()
                                    .map(|r| r.content.as_str())
                                    .collect::<Vec<&str>>()
                                    .join("\n");
                                if context_str.is_empty() {
                                    None
                                } else {
                                    Some(context_str)
                                }
                            } else {
                                None
                            }
                        } else {
                            None // Filler word ise RAG sorgusu atılmaz (Sıfır Gecikme)
                        };

                        let llama_req = GenerateStreamRequest {
                            system_prompt: system_prompt_id.clone(),
                            user_prompt: user_input.clone(),
                            rag_context,
                            history: history.clone(),
                            params: None,
                            lora_adapter_id: None,
                        };

                        let dialog_req = GenerateDialogStreamRequest {
                            model_selector: "local".to_string(),
                            tenant_id: tenant_id.clone(),
                            llama_request: Some(llama_req),
                        };

                        match llm_cli
                            .generate_stream(dialog_req, &trace_id, &span_id, &tenant_id)
                            .await
                        {
                            Ok(mut llm_stream) => {
                                let mut assistant_response = String::new();

                                while let Some(Ok(llm_resp)) = llm_stream.next().await {
                                    if let Some(resp_inner) = llm_resp.llama_response {
                                        match resp_inner.r#type {
                                            Some(LlmResponseType::Token(bytes)) => {
                                                let text_chunk =
                                                    String::from_utf8_lossy(&bytes).to_string();
                                                assistant_response.push_str(&text_chunk);

                                                if tx
                                                    .send(Ok(StreamConversationResponse {
                                                        payload: Some(RespPayload::TextResponse(
                                                            text_chunk,
                                                        )),
                                                    }))
                                                    .await
                                                    .is_err()
                                                {
                                                    // [ARCH-COMPLIANCE FIX]: Bu doğal bir "Barge-in" (Söz kesme)
                                                    // olayıdır. Sistemin çökmesi (Error) değil,
                                                    // kullanıcı kaynaklı bir iptal (Warn) sürecidir.
                                                    tracing::warn!(
                                                        event = "DIALOG_STREAM_CANCELLED",
                                                        trace_id = %trace_id,
                                                        span_id = %span_id,
                                                        "Client disconnected (Barge-in). Aborting LLM stream gracefully."
                                                    );
                                                    return;
                                                }
                                            }
                                            Some(LlmResponseType::FinishDetails(_)) => {}
                                            None => {}
                                        }
                                    }
                                }

                                // 5. Save History
                                history.push(ConversationTurn {
                                    role: "user".to_string(),
                                    content: user_input.clone(),
                                });
                                history.push(ConversationTurn {
                                    role: "assistant".to_string(),
                                    content: assistant_response.clone(),
                                });
                                state_mgr.save_history(&session_id, history).await;

                                // [EKLENDİ] Crystalline Service için 'dialog.turn.completed' olayını fırlat
                                let event_payload = json!({
                                    "session_id": session_id,
                                    "trace_id": trace_id,
                                    "user_input": user_input,
                                    "assistant_response": assistant_response
                                });
                                publisher
                                    .publish("dialog.turn.completed", event_payload)
                                    .await;

                                let _ = tx
                                    .send(Ok(StreamConversationResponse {
                                        payload: Some(RespPayload::IsFinalResponse(true)),
                                    }))
                                    .await;

                                info!(event = "DIALOG_TURN_COMPLETED", trace_id = %trace_id, span_id = %span_id, "Dialog turn finished and sent to MQ.");
                            }
                            Err(e) => {
                                error!(event = "LLM_REQUEST_FAILED", trace_id = %trace_id, span_id = %span_id, error = %e, "Failed to call LLM Gateway.");
                                let _ = tx.send(Err(Status::internal("LLM Engine failed"))).await;
                            }
                        }

                        accumulated_text.clear();
                    }
                    _ => {}
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
    }
}
