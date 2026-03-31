pub mod knowledge_client;
pub mod llm_client;

use crate::config::AppConfig;
use anyhow::{Context, Result};
use tonic::transport::{Certificate, ClientTlsConfig, Identity};

pub async fn load_client_tls_config(config: &AppConfig, domain: &str) -> Result<ClientTlsConfig> {
    let ca_cert = tokio::fs::read(&config.grpc_tls_ca_path)
        .await
        .context("Failed to read CA")?;
    let ca = Certificate::from_pem(ca_cert);

    let cert = tokio::fs::read(&config.dialog_service_cert_path).await?;
    let key = tokio::fs::read(&config.dialog_service_key_path).await?;
    let identity = Identity::from_pem(cert, key);

    Ok(ClientTlsConfig::new()
        .domain_name(domain)
        .ca_certificate(ca)
        .identity(identity))
}
