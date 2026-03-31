use axum::{response::Json, routing::get, Router};
use serde_json::json;
use std::net::SocketAddr;
use tracing::{error, info};

pub async fn start_health_server(addr: SocketAddr) {
    let app = Router::new().route(
        "/healthz",
        get(|| async { Json(json!({"status": "ok", "service": "dialog-service"})) }),
    );

    info!(event = "HTTP_SERVER_READY", address = %addr, "Health server listening.");
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    if let Err(e) = axum::serve(listener, app).await {
        error!(event = "HTTP_SERVER_CRASH", error = %e, "Health server failed");
    }
}
