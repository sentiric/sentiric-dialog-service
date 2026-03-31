mod app;
mod clients;
mod config;
mod logger;
mod server;
mod state;

use anyhow::Result;
use app::App;

#[tokio::main]
async fn main() -> Result<()> {
    // SUTS v4.0 uyumlu asenkron yapı başlatılır
    App::run().await
}
