use crate::clients::knowledge_client::KnowledgeClient;
use crate::clients::llm_client::LlmClient;
use crate::state::manager::StateManager;
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
    publisher: Arc<GhostPublisher>,
}

impl DialogServerImpl {
    pub fn new(
        state_manager: Arc<StateManager>,
        llm_client: Arc<LlmClient>,
        knowledge_client: Arc<KnowledgeClient>,
        publisher: Arc<GhostPublisher>,
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
        Err(Status::unimplemented(
            "Legacy start_dialog is deprecated. Use stream_conversation.",
        ))
    }

    async fn process_user_input(
        &self,
        _request: Request<ProcessUserInputRequest>,
    ) -> Result<Response<ProcessUserInputResponse>, Status> {
        Err(Status::unimplemented(
            "Legacy process_user_input is deprecated. Use stream_conversation.",
        ))
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
        let publisher = self.publisher.clone();

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

                        let history = state_mgr.get_history(&session_id).await;

                        // [ARCH-COMPLIANCE FIX]: Master Spec v4.0 RAG/Memory Retrieval Algoritması
                        let is_question = user_input.contains('?')
                            || user_input.to_lowercase().contains("kim")
                            || user_input.to_lowercase().contains("ne");
                        let word_count = user_input.split_whitespace().count();

                        // Soru soruluyorsa asla filler sayılmaz, kesinlikle hafızaya (Qdrant) gidilir.
                        let is_filler = !is_question && (word_count < 3 || user_input.len() < 15);
                        let is_system_command = user_input.contains("[SYSTEM");

                        let rag_context = if !user_input.is_empty()
                            && !is_filler
                            && !is_system_command
                        {
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
                            if is_filler {
                                tracing::debug!(event = "RAG_BYPASSED", trace_id = %trace_id, "Filler threshold met. No need to query memory.");
                            }
                            None
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
                                let mut in_action_text = false;

                                while let Some(Ok(llm_resp)) = llm_stream.next().await {
                                    if let Some(resp_inner) = llm_resp.llama_response {
                                        match resp_inner.r#type {
                                            Some(LlmResponseType::Token(bytes)) => {
                                                let raw_chunk =
                                                    String::from_utf8_lossy(&bytes).to_string();
                                                let mut clean_chunk = String::new();

                                                for c in raw_chunk.chars() {
                                                    if c == '*' {
                                                        in_action_text = !in_action_text;
                                                        continue;
                                                    }

                                                    if !in_action_text
                                                        && (c.is_alphanumeric()
                                                            || c.is_ascii_punctuation()
                                                            || c.is_whitespace())
                                                    {
                                                        clean_chunk.push(c);
                                                    }
                                                }

                                                assistant_response.push_str(&clean_chunk);

                                                if !clean_chunk.trim().is_empty()
                                                    && tx
                                                        .send(Ok(StreamConversationResponse {
                                                            payload: Some(
                                                                RespPayload::TextResponse(
                                                                    clean_chunk,
                                                                ),
                                                            ),
                                                        }))
                                                        .await
                                                        .is_err()
                                                {
                                                    tracing::warn!(event = "DIALOG_STREAM_CANCELLED", trace_id = %trace_id, "Client disconnected (Barge-in).");
                                                    return;
                                                }
                                            }
                                            Some(LlmResponseType::FinishDetails(_)) => {}
                                            None => {}
                                        }
                                    }
                                }

                                state_mgr
                                    .append_turns(
                                        &session_id,
                                        vec![
                                            ConversationTurn {
                                                role: "user".to_string(),
                                                content: user_input.clone(),
                                            },
                                            ConversationTurn {
                                                role: "assistant".to_string(),
                                                content: assistant_response.clone(),
                                            },
                                        ],
                                    )
                                    .await;

                                let event_payload = json!({
                                    "session_id": session_id,
                                    "trace_id": trace_id,
                                    "user_input": user_input,
                                    "assistant_response": assistant_response
                                });

                                publisher
                                    .publish_event(
                                        "dialog.turn.completed",
                                        &trace_id,
                                        event_payload,
                                    )
                                    .await;

                                let _ = tx
                                    .send(Ok(StreamConversationResponse {
                                        payload: Some(RespPayload::IsFinalResponse(true)),
                                    }))
                                    .await;

                                info!(event = "DIALOG_TURN_COMPLETED", trace_id = %trace_id, span_id = %span_id, "Dialog turn finished.");
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
