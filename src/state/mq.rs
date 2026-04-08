use crate::state::manager::StateManager;
use futures::StreamExt;
use lapin::{options::*, types::FieldTable, Connection, ConnectionProperties};
use serde_json::Value;
use std::sync::Arc;
use tracing::{info, warn};

pub struct ReflexConsumer;

impl ReflexConsumer {
    pub async fn start(rmq_url: String, state_mgr: Arc<StateManager>) {
        tokio::spawn(async move {
            loop {
                match Connection::connect(&rmq_url, ConnectionProperties::default()).await {
                    Ok(conn) => {
                        if let Ok(channel) = conn.create_channel().await {
                            let queue = channel
                                .queue_declare(
                                    "sentiric_dialog_reflex_queue",
                                    QueueDeclareOptions {
                                        durable: true,
                                        ..Default::default()
                                    },
                                    FieldTable::default(),
                                )
                                .await
                                .unwrap();

                            channel
                                .queue_bind(
                                    queue.name().as_str(),
                                    "sentiric_events",
                                    "cognitive.reflex.triggered",
                                    QueueBindOptions::default(),
                                    FieldTable::default(),
                                )
                                .await
                                .unwrap();

                            let mut consumer = channel
                                .basic_consume(
                                    queue.name().as_str(),
                                    "dialog_reflex_worker",
                                    BasicConsumeOptions::default(),
                                    FieldTable::default(),
                                )
                                .await
                                .unwrap();

                            info!(
                                event = "REFLEX_CONSUMER_READY",
                                "🧠 Listening for cognitive reflexes (Crystalline)..."
                            );

                            while let Some(delivery) = consumer.next().await {
                                if let Ok(delivery) = delivery {
                                    if let Ok(payload) =
                                        serde_json::from_slice::<Value>(&delivery.data)
                                    {
                                        let trace_id = payload["trace_id"].as_str().unwrap_or("");
                                        // [ARCH-COMPLIANCE FIX]: Dialog'un doğru session'ı bulması için eklendi
                                        let session_id =
                                            payload["session_id"].as_str().unwrap_or(trace_id);
                                        let payload_json_str =
                                            payload["payload_json"].as_str().unwrap_or("{}");

                                        if let Ok(insight) =
                                            serde_json::from_str::<Value>(payload_json_str)
                                        {
                                            let instruction =
                                                insight["instruction"].as_str().unwrap_or("");
                                            if !instruction.is_empty() && !session_id.is_empty() {
                                                info!(event = "INJECTING_REFLEX", session_id = %session_id, trace_id = %trace_id, "Applying cognitive modifier to session");
                                                state_mgr
                                                    .inject_reflex(
                                                        session_id,
                                                        instruction.to_string(),
                                                    )
                                                    .await;
                                            }
                                        }
                                    }
                                    let _ = delivery.ack(BasicAckOptions::default()).await;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        warn!(event = "MQ_CONNECT_FAIL", error = %e, "RabbitMQ unreachable for Reflex Consumer. Retrying in 5s...");
                    }
                }
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            }
        });
    }
}
