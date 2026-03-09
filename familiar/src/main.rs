mod bot;
mod config;
mod db;
mod embedding;
mod state;
mod tools;

use std::sync::Arc;

use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() {
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cfg = config::Config::from_env();

    info!("familiar starting");

    let db: db::Db = db::Db::open(&cfg.db_path)
        .await
        .unwrap_or_else(|e| panic!("failed to open database at {}: {e}", cfg.db_path));

    info!(path = %cfg.db_path, "database opened");

    let state = Arc::new(state::AppState::new(&cfg, db));

    bot::run(&cfg.discord_token, state).await;
}
