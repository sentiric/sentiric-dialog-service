// Dosya: sentiric-dialog-service/src/state/publisher.rs
use lapin::{options::*, BasicProperties, Connection, ConnectionProperties};
use serde_json::Value;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

// [ARCH-COMPLIANCE] Ghost Publisher Pattern: RabbitMQ yokluğunda In-Memory Ring Buffer (FIFO)
const MAX_RING_BUFFER_SIZE: usize = 1000;

pub struct GhostPublisher {
    rmq_url: String,
    connection: Arc<RwLock<Option<Connection>>>,
    ring_buffer: Arc<Mutex<VecDeque<(String, Vec<u8>)>>>, // (routing_key, payload)
}

impl GhostPublisher {
    pub async fn new(rmq_url: String) -> Self {
        let publisher = Self {
            rmq_url,
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
                    match Connection::connect(&url, ConnectionProperties::default()).await {
                        Ok(conn) => {
                            info!(event = "MQ_CONNECTED", "GhostPublisher connected to RabbitMQ.");
                            *conn_state.write().await = Some(conn);
                        }
                        Err(e) => {
                            warn!(event = "MQ_CONNECT_FAIL", error = %e, "RabbitMQ unreachable. Operating in Ghost Mode (Buffering).");
                            *conn_state.write().await = None;
                        }
                    }
                } else {
                    // Bağlantı kopukluğunu test et
                    if let Some(c) = conn_state.read().await.as_ref() {
                        if c.status().state() != lapin::ConnectionState::Connected {
                            warn!(event = "MQ_DISCONNECTED", "Connection dropped. Returning to Ghost Mode.");
                            *conn_state.write().await = None;
                        }
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
                            
                            // Tamponu boşalt
                            while let Some((routing_key, payload)) = buf_lock.pop_front() {
                                let result = channel.basic_publish(
                                    "sentiric_events",
                                    &routing_key,
                                    BasicPublishOptions::default(),
                                    &payload,
                                    BasicProperties::default(),
                                ).await;

                                if result.is_err() {
                                    // Gönderilemediyse geri koy ve kanalı kapat
                                    buf_lock.push_front((routing_key, payload));
                                    break; 
                                }
                            }
                            if buf_lock.is_empty() {
                                info!(event = "GHOST_BUFFER_DRAINED", "Ring buffer synced to RabbitMQ.");
                            }
                        }
                    }
                }
                sleep(Duration::from_secs(2)).await;
            }
        });
    }

    pub async fn publish(&self, routing_key: &str, payload: Value) {
        let json_payload = serde_json::to_vec(&payload).unwrap_or_default();
        let conn_opt = self.connection.read().await;

        if let Some(conn) = conn_opt.as_ref() {
            if let Ok(channel) = conn.create_channel().await {
                let res = channel.basic_publish(
                    "sentiric_events",
                    routing_key,
                    BasicPublishOptions::default(),
                    &json_payload,
                    BasicProperties::default(),
                ).await;

                if res.is_ok() {
                    return; // Başarılı
                }
            }
        }

        // Gönderilemediyse RAM'e al (Ring Buffer)
        let mut buf = self.ring_buffer.lock().await;
        if buf.len() >= MAX_RING_BUFFER_SIZE {
            buf.pop_front(); // FIFO (Eskileri sil)
            warn!(event = "GHOST_BUFFER_FULL", "Dropping oldest event. Ghost Publisher limit reached.");
        }
        buf.push_back((routing_key.to_string(), json_payload));
    }
}