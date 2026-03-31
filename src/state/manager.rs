// Hata E0277 düzeltmesi: Protobuf nesnelerini Redis'e yazmak için Local struct kullanımı
use crate::config::AppConfig;
use dashmap::DashMap;
use redis::aio::ConnectionManager;
use sentiric_contracts::sentiric::llm::v1::ConversationTurn;
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

#[derive(Serialize, Deserialize, Clone)]
pub struct LocalTurn {
    pub role: String,
    pub content: String,
}

pub struct StateManager {
    l1_cache: DashMap<String, Vec<LocalTurn>>,
    l2_redis: Option<ConnectionManager>,
}

impl StateManager {
    pub async fn new(config: &AppConfig) -> Self {
        let mut l2_redis = None;
        if !config.redis_url.is_empty() {
            match redis::Client::open(config.redis_url.as_str()) {
                Ok(client) => match client.get_connection_manager().await {
                    Ok(conn) => {
                        info!(
                            event = "REDIS_CONNECTED",
                            "L2 Cache (Redis) initialized successfully."
                        );
                        l2_redis = Some(conn);
                    }
                    Err(e) => {
                        warn!(event = "REDIS_CONNECT_FAIL", error = %e, "L2 Cache failed. Falling back to Nano-Edge (L1 Only).")
                    }
                },
                Err(e) => {
                    warn!(event = "REDIS_CLIENT_FAIL", error = %e, "Invalid Redis URL. Falling back to Nano-Edge.")
                }
            }
        } else {
            info!(
                event = "NANO_EDGE_MODE",
                "No Redis URL provided. Operating in Nano-Edge (L1 DashMap Only) mode."
            );
        }

        Self {
            l1_cache: DashMap::new(),
            l2_redis,
        }
    }

    pub async fn get_history(&self, session_id: &str) -> Vec<ConversationTurn> {
        // 1. Try L1
        if let Some(history) = self.l1_cache.get(session_id) {
            return history
                .iter()
                .map(|t| ConversationTurn {
                    role: t.role.clone(),
                    content: t.content.clone(),
                })
                .collect();
        }

        // 2. Try L2 if available
        if let Some(mut redis_conn) = self.l2_redis.clone() {
            let key = format!("dialog:session:{}", session_id);
            let result: redis::RedisResult<String> = redis::cmd("GET")
                .arg(&key)
                .query_async(&mut redis_conn)
                .await;

            if let Ok(json_str) = result {
                if let Ok(local_history) = serde_json::from_str::<Vec<LocalTurn>>(&json_str) {
                    self.l1_cache
                        .insert(session_id.to_string(), local_history.clone());

                    return local_history
                        .into_iter()
                        .map(|t| ConversationTurn {
                            role: t.role,
                            content: t.content,
                        })
                        .collect();
                }
            }
        }
        Vec::new()
    }

    pub async fn save_history(&self, session_id: &str, history: Vec<ConversationTurn>) {
        let local_history: Vec<LocalTurn> = history
            .into_iter()
            .map(|t| LocalTurn {
                role: t.role,
                content: t.content,
            })
            .collect();

        // 1. Save L1
        self.l1_cache
            .insert(session_id.to_string(), local_history.clone());

        // 2. Save L2
        if let Some(mut redis_conn) = self.l2_redis.clone() {
            let key = format!("dialog:session:{}", session_id);
            if let Ok(json_str) = serde_json::to_string(&local_history) {
                let _: redis::RedisResult<()> = redis::cmd("SETEX")
                    .arg(&key)
                    .arg(3600) // 1 Hour TTL
                    .arg(json_str)
                    .query_async(&mut redis_conn)
                    .await;
            } else {
                error!(event = "STATE_SERIALIZE_ERROR", session_id = %session_id, "Failed to serialize history for Redis");
            }
        }
    }
}
