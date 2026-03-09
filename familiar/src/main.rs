mod bot;
mod config;
mod state;
mod tools;

use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt};

#[tokio::main]
async fn main() {
    // ── Logging ───────────────────────────────────────────────────────────────
    fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // ── Config ────────────────────────────────────────────────────────────────
    let cfg = config::Config::from_env();

    info!(
        bot_token_len = cfg.telegram_token.len(),
        "agent-server starting"
    );

    // ── Shared state ──────────────────────────────────────────────────────────
    let state = Arc::new(state::AppState::new(&cfg));

    // ── Router ────────────────────────────────────────────────────────────────
    let app = Router::new()
        .route("/webhook", post(bot::webhook_handler))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    info!(addr, "listening");

    axum::serve(listener, app).await.unwrap();
}
