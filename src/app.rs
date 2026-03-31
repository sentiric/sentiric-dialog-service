use crate::clients::knowledge_client::KnowledgeClient;
use crate::clients::llm_client::LlmClient;
use crate::config::AppConfig;
use crate::logger::SutsV4Formatter;
use crate::server::grpc::DialogServerImpl;
use crate::server::http::start_health_server;
use crate::state::manager::StateManager;
use sentiric_contracts::sentiric::dialog::v1::dialog_service_server::DialogServiceServer;

use anyhow::{Context, Result};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tonic::transport::{Certificate, Identity, Server, ServerTlsConfig};
use tracing::{error, info}; // Artık her ikisi de koda dahil

pub struct App;

impl App {
    pub async fn run() -> Result<()> {
        let config = Arc::new(AppConfig::load()?);

        let formatter = SutsV4Formatter {
            service_name: "dialog-service".to_string(),
            service_version: config.service_version.clone(),
            service_env: config.env.clone(),
        };

        tracing_subscriber::fmt()
            .with_env_filter(&config.rust_log)
            .event_format(formatter)
            .init();

        info!(
            event = "SERVICE_START",
            "🚀 Dialog Service booting up (mTLS Strict, Nano-Edge Ready)..."
        );

        let state_manager = Arc::new(StateManager::new(&config).await);
        let llm_client = Arc::new(LlmClient::new(&config).await?);
        let knowledge_client = Arc::new(KnowledgeClient::new(&config).await?);

        let http_addr: SocketAddr = format!("{}:{}", config.host, config.http_port).parse()?;
        tokio::spawn(async move {
            start_health_server(http_addr).await;
        });

        let grpc_addr: SocketAddr = format!("{}:{}", config.host, config.grpc_port).parse()?;
        let dialog_impl = DialogServerImpl::new(state_manager, llm_client, knowledge_client);

        let cert = tokio::fs::read(&config.dialog_service_cert_path)
            .await
            .context("Failed to read cert")?;
        let key = tokio::fs::read(&config.dialog_service_key_path)
            .await
            .context("Failed to read key")?;
        let ca_cert = tokio::fs::read(&config.grpc_tls_ca_path)
            .await
            .context("Failed to read CA")?;

        let identity = Identity::from_pem(cert, key);
        let client_ca_root = Certificate::from_pem(ca_cert);

        let tls_config = ServerTlsConfig::new()
            .identity(identity)
            .client_ca_root(client_ca_root);

        info!(event = "GRPC_SERVER_READY", address = %grpc_addr, "🎧 gRPC Dialog Server listening (mTLS Enabled)");

        if let Err(e) = Server::builder()
            .tls_config(tls_config)?
            .add_service(DialogServiceServer::new(dialog_impl))
            .serve_with_shutdown(grpc_addr, shutdown_signal())
            .await
        {
            error!(event = "GRPC_SERVER_CRASH", error = %e, "Server crashed unexpectedly");
            return Err(e.into());
        }

        info!(
            event = "SERVICE_STOPPED",
            "🛑 Dialog Service stopped gracefully."
        );
        Ok(())
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
