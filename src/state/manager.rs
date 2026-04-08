// File: sentiric-dialog-service/src/state/manager.rs
// [ARCH-COMPLIANCE] Auto-Healing, Graceful Degradation and L1->L2 Cache Synchronization Implemented
use crate::config::AppConfig;
use dashmap::DashMap;
use redis::aio::ConnectionManager;
use sentiric_contracts::sentiric::llm::v1::ConversationTurn;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep; // [FIX 1] E0425 Çözümü: sleep eklendi
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
        // [FIX 2] E0282 Çözümü: DashMap Key(String) ve Value(Vec<LocalTurn>) açıkça belirtildi
        let l1_cache: Arc<DashMap<String, Vec<LocalTurn>>> = Arc::new(DashMap::new());
        let l2_redis = Arc::new(RwLock::new(None));

        if !config.redis_url.is_empty() {
            let redis_url = config.redis_url.clone();
            let l2_redis_clone = l2_redis.clone();
            let l1_cache_clone = l1_cache.clone();

            match redis::Client::open(redis_url.as_str()) {
                Ok(client) => {
                    let mut initial_connected = false;

                    // İlk bağlantı denemesi
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

                    // Arka Plan Görevi: Auto-Healing & Health Check
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
                                        backoff = 1; // Başarılı ping'de backoff sıfırlanır
                                        if !was_connected {
                                            info!(event = "REDIS_RECONNECTED", "L2 Cache (Redis) is back online. Switching to Nano-Edge L1+L2 mode.");
                                            *l2_redis_clone.write().await = Some(m.clone());
                                            was_connected = true;

                                            // [ARCH-COMPLIANCE] L1 -> L2 Cache Drift Sync (Flush)
                                            let sync_cache = l1_cache_clone.clone();
                                            let mut sync_conn = m.clone();

                                            tokio::spawn(async move {
                                                // [MİMARİ GÜVENLİK FIX] DashMap iteratörü kilit (read lock) tutar.
                                                // .await noktası üzerinden (cross-await) lock tutmamak ve Send hatası almamak için
                                                // verinin önce hızlıca RAM'de kopyası (snapshot) alınıyor, lock derhal bırakılıyor.
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
        // 1. Try L1 (Hot Memory)
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

        // 2. Try L2 if available
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

        // 2. Save L2 (Yalnızca Redis ayaktaysa yazılır)
        let l2_conn_opt = self.l2_redis.read().await.clone();
        if let Some(mut redis_conn) = l2_conn_opt {
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

    // StateManager impl bloğu içerisine ekleyin:
    pub async fn inject_reflex(&self, session_id: &str, instruction: String) {
        let mut history = self.get_history(session_id).await;
        // Fısıltı (Insight) olarak sistemi bilgilendirir
        history.push(ConversationTurn {
            role: "system".to_string(),
            content: format!("[COGNITIVE_REFLEX]: {}", instruction),
        });
        self.save_history(session_id, history).await;
    }
}
