// File: src/clients/llm_client.rs
use crate::clients::load_client_tls_config;
use crate::config::AppConfig;
use sentiric_contracts::sentiric::llm::v1::llm_gateway_service_client::LlmGatewayServiceClient;
use sentiric_contracts::sentiric::llm::v1::{
    GenerateDialogStreamRequest, GenerateDialogStreamResponse,
};
use std::str::FromStr;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};
use tonic::Request;
use tracing::{error, info};

#[derive(Clone)]
pub struct LlmClient {
    client: LlmGatewayServiceClient<Channel>,
}

impl LlmClient {
    pub async fn new(config: &AppConfig) -> anyhow::Result<Self> {
        let url = config.llm_gateway_service_target.clone();
        if url.starts_with("http://") {
            panic!("⚠️ [ARCH-COMPLIANCE] Insecure connection to LLM Gateway is FORBIDDEN.");
        }

        info!(event = "UPSTREAM_CONNECTING", target = %url, "🔐 Connecting to LLM Gateway (mTLS - Lazy)");

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
            client: LlmGatewayServiceClient::new(channel),
        })
    }

    pub async fn generate_stream(
        &self,
        request: GenerateDialogStreamRequest,
        trace_id: &str,
        span_id: &str,
        tenant_id: &str,
    ) -> Result<tonic::Streaming<GenerateDialogStreamResponse>, tonic::Status> {
        let mut client = self.client.clone();
        let mut req = Request::new(request);

        if let Ok(meta_val) = MetadataValue::from_str(trace_id) {
            req.metadata_mut().insert("x-trace-id", meta_val);
        }
        if let Ok(meta_val) = MetadataValue::from_str(span_id) {
            req.metadata_mut().insert("x-span-id", meta_val);
        }
        if let Ok(meta_val) = MetadataValue::from_str(tenant_id) {
            req.metadata_mut().insert("x-tenant-id", meta_val);
        }

        match client.generate_dialog_stream(req).await {
            Ok(response) => Ok(response.into_inner()),
            Err(e) => {
                error!(event = "UPSTREAM_CALL_FAILED", trace_id = %trace_id, span_id = %span_id, tenant_id = %tenant_id, error = %e, "❌ LLM Gateway gRPC call failed");
                Err(e)
            }
        }
    }
}
