use anyhow::Result;
use config::{Config, Environment, File};
use serde::Deserialize;
use std::env;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub env: String,
    pub rust_log: String,
    pub service_version: String,
    #[allow(dead_code)]
    pub tenant_id: String,

    pub host: String,
    pub grpc_port: u16,
    pub http_port: u16,

    pub redis_url: String,
    pub llm_gateway_service_target: String,
    pub knowledge_query_service_target: String,

    pub grpc_tls_ca_path: String,
    pub dialog_service_cert_path: String,
    pub dialog_service_key_path: String,
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        let builder = Config::builder()
            .add_source(File::with_name(".env").required(false))
            .add_source(Environment::default().separator("__"))
            .set_override_option("host", env::var("DIALOG_SERVICE_LISTEN_ADDRESS").ok())?
            .set_override_option("grpc_port", env::var("DIALOG_SERVICE_GRPC_PORT").ok())?
            .set_override_option("http_port", env::var("DIALOG_SERVICE_HTTP_PORT").ok())?
            .set_override_option("redis_url", env::var("REDIS_URL").ok())?
            .set_override_option(
                "llm_gateway_service_target",
                env::var("LLM_GATEWAY_SERVICE_TARGET").ok(),
            )?
            .set_override_option(
                "knowledge_query_service_target",
                env::var("KNOWLEDGE_QUERY_SERVICE_GRPC_URL").ok(),
            )?
            .set_default("env", "production")?
            .set_default("rust_log", "info")?
            .set_default("service_version", env!("CARGO_PKG_VERSION"))?
            .set_default("tenant_id", "sentiric_demo")?
            .set_default("host", "0.0.0.0")?
            .set_default("grpc_port", 12061)?
            .set_default("http_port", 12060)?
            .set_default("redis_url", "")?
            .set_default(
                "llm_gateway_service_target",
                "https://llm-gateway-service:16021",
            )?
            .set_default(
                "knowledge_query_service_target",
                "https://knowledge-query-service:17021",
            )?
            .set_default("grpc_tls_ca_path", "/sentiric-certificates/certs/ca.crt")?
            .set_default(
                "dialog_service_cert_path",
                "/sentiric-certificates/certs/dialog-service-chain.crt",
            )?
            .set_default(
                "dialog_service_key_path",
                "/sentiric-certificates/certs/dialog-service.key",
            )?;

        let config: AppConfig = builder.build()?.try_deserialize()?;

        if config.dialog_service_cert_path.is_empty() || config.grpc_tls_ca_path.is_empty() {
            panic!("⚠️ [ARCH-COMPLIANCE] mTLS certificates are MANDATORY. Insecure fallback is FORBIDDEN.");
        }

        Ok(config)
    }
}
