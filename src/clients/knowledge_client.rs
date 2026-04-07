// File: src/clients/knowledge_client.rs
use crate::clients::load_client_tls_config;
use crate::config::AppConfig;
use sentiric_contracts::sentiric::knowledge::v1::knowledge_query_service_client::KnowledgeQueryServiceClient;
use sentiric_contracts::sentiric::knowledge::v1::{QueryRequest, QueryResponse};
use std::str::FromStr;
use std::time::Duration;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use tonic::Request;
use tracing::{info, warn};

#[derive(Clone)]
pub struct KnowledgeClient {
    client: Option<KnowledgeQueryServiceClient<Channel>>,
}

impl KnowledgeClient {
    pub async fn new(config: &AppConfig) -> anyhow::Result<Self> {
        let url = config
            .knowledge_query_service_target
            .trim()
            .replace("\"", "");

        if url.is_empty() {
            info!(
                event = "RAG_DISABLED",
                "Knowledge Query URL is empty. Operating in RAG-less Nano-Edge mode."
            );
            return Ok(Self { client: None });
        }

        if url.starts_with("http://") {
            panic!("⚠️ [ARCH-COMPLIANCE] Insecure connection to Knowledge Query is FORBIDDEN.");
        }

        info!(event = "UPSTREAM_CONNECTING", target = %url, "🔐 Connecting to Knowledge Query (mTLS - Lazy)");

        let domain = url
            .replace("https://", "")
            .split(':')
            .next()
            .unwrap_or("sentiric.cloud")
            .to_string();
        let tls_config = load_client_tls_config(config, &domain).await?;

        // [ARCH-COMPLIANCE FIX] .connect().await? yerine .connect_lazy() kullanılarak
        // DNS yarış durumları (Consul) ve başlangıç çökmesi engellendi.
        let channel = Endpoint::from_shared(url)?
            .tls_config(tls_config)?
            .connect_lazy();

        Ok(Self {
            client: Some(KnowledgeQueryServiceClient::new(channel)),
        })
    }

    pub async fn query(
        &self,
        tenant_id: &str,
        query: &str,
        trace_id: &str,
        span_id: &str,
    ) -> Option<QueryResponse> {
        let mut client = match &self.client {
            Some(c) => c.clone(),
            None => return None,
        };

        let request_payload = QueryRequest {
            tenant_id: tenant_id.to_string(),
            query: query.to_string(),
            top_k: 3,
        };

        let mut req = Request::new(request_payload);
        req.set_timeout(Duration::from_secs(3));

        if let Ok(meta_val) = MetadataValue::from_str(trace_id) {
            req.metadata_mut().insert("x-trace-id", meta_val);
        }
        if let Ok(meta_val) = MetadataValue::from_str(span_id) {
            req.metadata_mut().insert("x-span-id", meta_val);
        }
        if let Ok(meta_val) = MetadataValue::from_str(tenant_id) {
            req.metadata_mut().insert("x-tenant-id", meta_val);
        }

        match client.query(req).await {
            Ok(response) => Some(response.into_inner()),
            Err(e) => {
                warn!(event = "RAG_QUERY_FAILED", trace_id = %trace_id, span_id = %span_id, error = %e, "Knowledge Query failed or timed out. Proceeding without RAG context.");
                None
            }
        }
    }
}
