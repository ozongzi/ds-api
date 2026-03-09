//! Telegram webhook handler.
//!
//! Responsibilities:
//! - Deserialize incoming `Update` objects from Telegram.
//! - Verify the `X-Telegram-Bot-Api-Secret-Token` header when configured.
//! - Dispatch text messages to the agent and stream the reply back via
//!   `sendMessage` (one message per completed agent turn).
//! - Expose `send_message` as a thin async helper used elsewhere.

use std::sync::Arc;

use axum::Json;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::IntoResponse;
use ds_api::AgentEvent;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};

use crate::state::AppState;

// ── Telegram types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct Update {
    pub update_id: i64,
    pub message: Option<Message>,
}

#[derive(Debug, Deserialize)]
pub struct Message {
    pub message_id: i64,
    pub chat: Chat,
    pub text: Option<String>,
    pub from: Option<User>,
}

#[derive(Debug, Deserialize)]
pub struct Chat {
    pub id: i64,
}

#[derive(Debug, Deserialize)]
pub struct User {
    pub id: i64,
    pub first_name: String,
    pub username: Option<String>,
}

#[derive(Debug, Serialize)]
struct SendMessageRequest<'a> {
    chat_id: i64,
    text: &'a str,
    /// Use "Markdown" for simple formatting; fall back to plain text if the
    /// message contains characters that would break Markdown parsing.
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<&'a str>,
}

// ── Webhook handler ───────────────────────────────────────────────────────────

/// axum handler for `POST /webhook`.
///
/// Telegram calls this endpoint for every update.  We respond with `200 OK`
/// immediately (Telegram requires a fast acknowledgement) and run the agent
/// turn in a spawned task so we never block Telegram's delivery loop.
pub async fn webhook_handler(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(update): Json<Update>,
) -> impl IntoResponse {
    // ── Secret token verification ─────────────────────────────────────────────
    if let Some(expected) = &state.webhook_secret {
        let provided = headers
            .get("X-Telegram-Bot-Api-Secret-Token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if provided != expected {
            warn!("webhook request with wrong secret token — ignoring");
            // Return 200 so Telegram doesn't retry, but do nothing.
            return StatusCode::OK;
        }
    }

    debug!(update_id = update.update_id, "received update");

    // Only handle text messages; silently ignore everything else (stickers,
    // voice, etc.).
    let Some(msg) = update.message else {
        return StatusCode::OK;
    };
    let Some(text) = msg.text else {
        return StatusCode::OK;
    };

    let chat_id = msg.chat.id;
    let user = msg
        .from
        .as_ref()
        .map(|u| u.first_name.as_str())
        .unwrap_or("user");
    info!(chat_id, user, "incoming message");

    // Spawn the agent turn so we can return 200 immediately.
    tokio::spawn(handle_turn(state, chat_id, text));

    StatusCode::OK
}

// ── Agent turn ────────────────────────────────────────────────────────────────

/// Drive one agent turn for `chat_id` and send the reply back via Telegram.
///
/// The agent is `take()`-n out of the shared state while the turn runs, then
/// put back when the stream finishes.  Concurrent messages from the same chat
/// are serialized via the interrupt channel: the second message arrives while
/// the first turn is still running and is injected mid-loop via
/// `interrupt_tx.send()`.
async fn handle_turn(state: Arc<AppState>, chat_id: i64, text: String) {
    // Try to take the agent out of the shared map.
    //
    // If another turn is already running for this chat (agent is None), inject
    // the new message through the interrupt channel instead of starting a new
    // turn — the running turn will pick it up after its current tool round.
    let agent_opt = state.with_chat(chat_id, |entry| {
        if entry.agent.is_some() {
            entry.agent.take()
        } else {
            // Agent is busy — inject via interrupt channel.
            let _ = entry.interrupt_tx.send(text.clone());
            None
        }
    });

    let Some(agent) = agent_opt else {
        info!(
            chat_id,
            "agent busy — message injected via interrupt channel"
        );
        return;
    };

    // Drive the agent turn.
    let mut reply = String::new();
    let mut stream = agent.chat(&text);

    while let Some(event) = stream.next().await {
        match event {
            Ok(AgentEvent::Token(token)) => {
                reply.push_str(&token);
            }
            Ok(AgentEvent::ToolCall(info)) => {
                info!(chat_id, tool = info.name, "tool call");
            }
            Ok(AgentEvent::ToolResult(res)) => {
                info!(chat_id, tool = res.name, "tool result");
            }
            Err(e) => {
                error!(chat_id, error = %e, "agent error");
                let _ = send_message(
                    &state.telegram_token,
                    chat_id,
                    "⚠️ Something went wrong. Please try again.",
                )
                .await;
                // Put the agent back even on error so the chat remains usable.
                if let Some(recovered) = stream.into_agent() {
                    state.with_chat(chat_id, |entry| entry.agent = Some(recovered));
                }
                return;
            }
        }
    }

    // Recover the agent and put it back before sending the reply, so a
    // follow-up message that arrives while sendMessage is in flight can already
    // acquire the agent.
    if let Some(recovered) = stream.into_agent() {
        state.with_chat(chat_id, |entry| entry.agent = Some(recovered));
    }

    if reply.is_empty() {
        return;
    }

    if let Err(e) = send_message(&state.telegram_token, chat_id, &reply).await {
        error!(chat_id, error = %e, "failed to send Telegram message");
    }
}

// ── Telegram API helper ───────────────────────────────────────────────────────

/// Call `sendMessage` on the Telegram Bot API.
///
/// Tries Markdown parse mode first; if Telegram rejects it (parse error) falls
/// back to plain text so the user always gets a reply.
pub async fn send_message(token: &str, chat_id: i64, text: &str) -> Result<(), reqwest::Error> {
    let url = format!("https://api.telegram.org/bot{token}/sendMessage");
    let client = reqwest::Client::new();

    // Split long messages — Telegram's limit is 4096 characters per message.
    for chunk in split_message(text, 4096) {
        let body = SendMessageRequest {
            chat_id,
            text: chunk,
            parse_mode: Some("Markdown"),
        };

        let resp = client.post(&url).json(&body).send().await?;

        // If Telegram rejects Markdown (e.g. unclosed formatting), retry as plain text.
        if !resp.status().is_success() {
            let plain = SendMessageRequest {
                chat_id,
                text: chunk,
                parse_mode: None,
            };
            client.post(&url).json(&plain).send().await?;
        }
    }

    Ok(())
}

/// Split `text` into chunks of at most `max_chars` characters, breaking on
/// newlines where possible to avoid splitting mid-sentence.
fn split_message(text: &str, max_chars: usize) -> Vec<&str> {
    if text.len() <= max_chars {
        return vec![text];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let end = (start + max_chars).min(text.len());

        // Try to break on a newline within the last 20% of the chunk.
        let search_from = start + (max_chars * 4 / 5);
        let break_at = if end < text.len() {
            text[search_from..end]
                .rfind('\n')
                .map(|i| search_from + i + 1)
                .unwrap_or(end)
        } else {
            end
        };

        chunks.push(&text[start..break_at]);
        start = break_at;
    }

    chunks
}
