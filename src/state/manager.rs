// File: sentiric-dialog-service/src/state/manager.rs
use crate::config::AppConfig;
use dashmap::DashMap;
use redis::aio::ConnectionManager;
use sentiric_contracts::sentiric::llm::v1::ConversationTurn;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;
use tracing::{error, info, warn};

#[derive(Serialize, Deserialize, Clone)]
pub struct LocalTurn {
    pub role: String,
    pub content: String,
}

pub struct StateManager {
    l1_cache: Arc<DashMap<String, Vec<LocalTurn>>>,
    l2_redis: Arc<RwLock<Option<ConnectionManager>>>,
}

impl StateManager {
    pub async fn new(config: &AppConfig) -> Self {
        let l1_cache: Arc<DashMap<String, Vec<LocalTurn>>> = Arc::new(DashMap::new());
        let l2_redis = Arc::new(RwLock::new(None));

        if !config.redis_url.is_empty() {
            let redis_url = config.redis_url.clone();
            let l2_redis_clone = l2_redis.clone();
            let l1_cache_clone = l1_cache.clone();

            match redis::Client::open(redis_url.as_str()) {
                Ok(client) => {
                    let mut initial_connected = false;

                    match client.get_connection_manager().await {
                        Ok(conn) => {
                            info!(
                                event = "REDIS_CONNECTED",
                                "L2 Cache (Redis) initialized successfully."
                            );
                            *l2_redis.write().await = Some(conn);
                            initial_connected = true;
                        }
                        Err(e) => {
                            warn!(event = "REDIS_CONNECT_FAIL", error = %e, "L2 Cache failed. Falling back to Nano-Edge (L1 Only).")
                        }
                    }

                    tokio::spawn(async move {
                        let mut manager: Option<ConnectionManager> = None;
                        let mut was_connected = initial_connected;
                        let mut backoff = 1;
                        const MAX_BACKOFF: u64 = 60;

                        loop {
                            sleep(Duration::from_secs(backoff)).await;

                            if manager.is_none() {
                                if let Ok(m) = client.get_connection_manager().await {
                                    manager = Some(m);
                                }
                            }

                            if let Some(mut m) = manager.clone() {
                                let ping_result: redis::RedisResult<String> =
                                    redis::cmd("PING").query_async(&mut m).await;

                                match ping_result {
                                    Ok(_) => {
                                        backoff = 1;
                                        if !was_connected {
                                            info!(event = "REDIS_RECONNECTED", "L2 Cache (Redis) is back online. Switching to Nano-Edge L1+L2 mode.");
                                            *l2_redis_clone.write().await = Some(m.clone());
                                            was_connected = true;

                                            let sync_cache = l1_cache_clone.clone();
                                            let mut sync_conn = m.clone();

                                            tokio::spawn(async move {
                                                let mut cache_snapshot = Vec::new();
                                                for entry in sync_cache.iter() {
                                                    cache_snapshot.push((
                                                        entry.key().clone(),
                                                        entry.value().clone(),
                                                    ));
                                                }

                                                let mut synced_count = 0;
                                                for (session_id, history) in cache_snapshot {
                                                    let key =
                                                        format!("dialog:session:{}", session_id);
                                                    if let Ok(json_str) =
                                                        serde_json::to_string(&history)
                                                    {
                                                        let _: redis::RedisResult<()> =
                                                            redis::cmd("SETEX")
                                                                .arg(&key)
                                                                .arg(3600)
                                                                .arg(json_str)
                                                                .query_async(&mut sync_conn)
                                                                .await;
                                                        synced_count += 1;
                                                    }
                                                }
                                                if synced_count > 0 {
                                                    info!(event = "L1_L2_SYNC_COMPLETE", synced_items = synced_count, "Ghost mode history flushed to Redis L2 Cache.");
                                                }
                                            });
                                        }
                                    }
                                    Err(e) => {
                                        if was_connected {
                                            warn!(event = "REDIS_DROPPED", error = %e, "L2 Cache (Redis) connection lost. Falling back to Nano-Edge (L1 Only).");
                                            *l2_redis_clone.write().await = None;
                                            was_connected = false;
                                        }
                                        backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
                                    }
                                }
                            } else {
                                backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
                            }
                        }
                    });
                }
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

        Self { l1_cache, l2_redis }
    }

    pub async fn get_history(&self, session_id: &str) -> Vec<ConversationTurn> {
        if let Some(history) = self.l1_cache.get(session_id) {
            return history
                .value()
                .iter()
                .map(|t| ConversationTurn {
                    role: t.role.clone(),
                    content: t.content.clone(),
                })
                .collect();
        }

        let l2_conn_opt = self.l2_redis.read().await.clone();
        if let Some(mut redis_conn) = l2_conn_opt {
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

    // [ARCH-COMPLIANCE FIX]: Race Condition önlemek için Atomic Append eklendi.
    pub async fn append_turns(&self, session_id: &str, turns: Vec<ConversationTurn>) {
        let mut local_turns: Vec<LocalTurn> = turns
            .into_iter()
            .map(|t| LocalTurn {
                role: t.role,
                content: t.content,
            })
            .collect();

        // 1. L1 Cache Atomic Push (Hot Memory)
        let updated_history = {
            // Clippy Hatası Çözümü: or_insert_with(Vec::new) yerine or_default() kullanıldı.
            let mut entry = self.l1_cache.entry(session_id.to_string()).or_default();
            entry.append(&mut local_turns);
            entry.clone()
        };

        // 2. L2 Cache Update
        let l2_conn_opt = self.l2_redis.read().await.clone();
        if let Some(mut redis_conn) = l2_conn_opt {
            let key = format!("dialog:session:{}", session_id);
            if let Ok(json_str) = serde_json::to_string(&updated_history) {
                let _: redis::RedisResult<()> = redis::cmd("SETEX")
                    .arg(&key)
                    .arg(3600)
                    .arg(json_str)
                    .query_async(&mut redis_conn)
                    .await;
            } else {
                error!(event = "STATE_SERIALIZE_ERROR", session_id = %session_id, "Failed to serialize history for Redis");
            }
        }
    }

    pub async fn inject_reflex(&self, session_id: &str, instruction: String) {
        self.append_turns(
            session_id,
            vec![ConversationTurn {
                role: "system".to_string(),
                content: format!("[COGNITIVE_REFLEX]: {}", instruction),
            }],
        )
        .await;
    }
}
