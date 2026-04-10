use chrono::Utc;
use lapin::{options::*, BasicProperties, Connection, ConnectionProperties};
use prost::Message;
use sentiric_contracts::sentiric::event::v1::GenericEvent;
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

type RmqPayload = (String, Vec<u8>);
type SharedRingBuffer = Arc<Mutex<VecDeque<RmqPayload>>>;

const MAX_RING_BUFFER_SIZE: usize = 1000;

pub struct GhostPublisher {
    rmq_url: String,
    tenant_id: String,
    connection: Arc<RwLock<Option<Connection>>>,
    ring_buffer: SharedRingBuffer,
}

impl GhostPublisher {
    pub async fn new(rmq_url: String, tenant_id: String) -> Self {
        let publisher = Self {
            rmq_url,
            tenant_id,
            connection: Arc::new(RwLock::new(None)),
            ring_buffer: Arc::new(Mutex::new(VecDeque::with_capacity(MAX_RING_BUFFER_SIZE))),
        };

        publisher.start_connection_manager().await;
        publisher.start_drain_worker().await;

        publisher
    }

    async fn start_connection_manager(&self) {
        let conn_state = self.connection.clone();
        let url = self.rmq_url.clone();

        tokio::spawn(async move {
            loop {
                let is_connected = conn_state.read().await.is_some();
                if !is_connected {
                    if url.trim().is_empty() {
                        warn!(
                            event = "MQ_DISABLED",
                            "RabbitMQ URL is empty. Running in Ghost mode only."
                        );
                        sleep(Duration::from_secs(60)).await;
                        continue;
                    }

                    match Connection::connect(&url, ConnectionProperties::default()).await {
                        Ok(conn) => {
                            info!(
                                event = "MQ_CONNECTED",
                                "GhostPublisher connected to RabbitMQ."
                            );
                            *conn_state.write().await = Some(conn);
                        }
                        Err(e) => {
                            warn!(event = "MQ_CONNECT_FAIL", error = %e, "RabbitMQ unreachable. Operating in Ghost Mode (Buffering).");
                            *conn_state.write().await = None;
                        }
                    }
                } else {
                    let mut should_clear = false;
                    if let Some(c) = conn_state.read().await.as_ref() {
                        if c.status().state() != lapin::ConnectionState::Connected {
                            warn!(
                                event = "MQ_DISCONNECTED",
                                "Connection dropped. Returning to Ghost Mode."
                            );
                            should_clear = true;
                        }
                    }
                    if should_clear {
                        *conn_state.write().await = None;
                    }
                }
                sleep(Duration::from_secs(5)).await;
            }
        });
    }

    async fn start_drain_worker(&self) {
        let conn_state = self.connection.clone();
        let buffer = self.ring_buffer.clone();

        tokio::spawn(async move {
            loop {
                let has_conn = conn_state.read().await.is_some();
                let has_items = !buffer.lock().await.is_empty();

                if has_conn && has_items {
                    if let Some(conn) = conn_state.read().await.as_ref() {
                        if let Ok(channel) = conn.create_channel().await {
                            let mut buf_lock = buffer.lock().await;

                            while let Some((routing_key, payload)) = buf_lock.pop_front() {
                                let result = channel
                                    .basic_publish(
                                        "sentiric_events",
                                        &routing_key,
                                        BasicPublishOptions::default(),
                                        &payload,
                                        BasicProperties::default(),
                                    )
                                    .await;

                                if result.is_err() {
                                    buf_lock.push_front((routing_key, payload));
                                    break;
                                }
                            }
                            if buf_lock.is_empty() {
                                info!(
                                    event = "GHOST_BUFFER_DRAINED",
                                    "Ring buffer synced to RabbitMQ."
                                );
                            }
                        }
                    }
                }
                sleep(Duration::from_secs(2)).await;
            }
        });
    }

    pub async fn publish_event(&self, event_type: &str, trace_id: &str, payload: Value) {
        let payload_json = payload.to_string();

        let event = GenericEvent {
            event_type: event_type.to_string(),
            trace_id: trace_id.to_string(),
            timestamp: Some(prost_types::Timestamp {
                seconds: Utc::now().timestamp(),
                nanos: Utc::now().timestamp_subsec_nanos() as i32,
            }),
            tenant_id: self.tenant_id.clone(),
            payload_json,
        };

        let mut bin_payload = Vec::new();
        if event.encode(&mut bin_payload).is_err() {
            warn!(
                event = "EVENT_ENCODE_FAIL",
                "Failed to encode GenericEvent using Protobuf."
            );
            return;
        }

        let conn_opt = self.connection.read().await;

        if let Some(conn) = conn_opt.as_ref() {
            if let Ok(channel) = conn.create_channel().await {
                let res = channel
                    .basic_publish(
                        "sentiric_events",
                        event_type,
                        BasicPublishOptions::default(),
                        &bin_payload,
                        BasicProperties::default(),
                    )
                    .await;

                if res.is_ok() {
                    return;
                }
            }
        }

        let mut buf = self.ring_buffer.lock().await;
        if buf.len() >= MAX_RING_BUFFER_SIZE {
            buf.pop_front();
            warn!(
                event = "GHOST_BUFFER_FULL",
                "Dropping oldest event. Ghost Publisher limit reached."
            );
        }
        buf.push_back((event_type.to_string(), bin_payload));
    }
}
