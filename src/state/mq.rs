// File: sentiric-dialog-service/src/state/mq.rs
use crate::state::manager::StateManager;
use futures::StreamExt;
use lapin::{options::*, types::FieldTable, Connection, ConnectionProperties};
use prost::Message; // [YENİ] Protobuf decode için
use sentiric_contracts::sentiric::event::v1::GenericEvent; // [YENİ]
use serde_json::Value;
use std::sync::Arc;
use tracing::{error, info, warn};

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
                                    // [ARCH-COMPLIANCE FIX]: JSON yerine GenericEvent (Protobuf) Decode
                                    match GenericEvent::decode(&*delivery.data) {
                                        Ok(event_data) => {
                                            let trace_id = event_data.trace_id;
                                            let payload_json_str = event_data.payload_json;

                                            if let Ok(insight) =
                                                serde_json::from_str::<Value>(&payload_json_str)
                                            {
                                                let instruction =
                                                    insight["instruction"].as_str().unwrap_or("");
                                                // GenericEvent'te session_id olmadığı için trace_id'yi session_id olarak varsayıyoruz.
                                                // Crystalline zaten call_id boşsa trace_id basıyordu.
                                                let session_id = trace_id.clone();

                                                if !instruction.is_empty() && !session_id.is_empty()
                                                {
                                                    info!(event = "INJECTING_REFLEX", session_id = %session_id, trace_id = %trace_id, "Applying cognitive modifier to session");
                                                    state_mgr
                                                        .inject_reflex(
                                                            &session_id,
                                                            instruction.to_string(),
                                                        )
                                                        .await;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            error!(event = "PROTO_UNMARSHAL_FAIL", error = %e, "Failed to decode GenericEvent for Reflex");
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
