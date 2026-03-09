//! Discord event handler and per-turn persistence.
//!
//! # Message flow
//!
//! ```text
//! user sends message in channel
//!   │
//!   ▼
//! Handler::message()
//!   ├─ ignore bots / empty messages
//!   ├─ post a "thinking…" placeholder message
//!   └─ spawn handle_turn()
//!         │
//!         ├─ agent.chat() → AgentStream
//!         │     ├─ Token        → accumulate; edit placeholder every ~800ms
//!         │     ├─ ToolCall     → post grey embed with spoiler-wrapped args
//!         │     └─ ToolResult   → edit that embed green/red with spoiler-wrapped result
//!         └─ final edit with complete reply
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use ds_api::AgentEvent;
use futures::StreamExt;
use serenity::all::{
    CreateAttachment, CreateEmbed, CreateMessage, EditMessage, GatewayIntents, Http,
};
use serenity::async_trait;
use serenity::model::channel::Message as DiscordMessage;
use serenity::model::gateway::Ready;
use serenity::prelude::*;
use tracing::{error, info};

use crate::state::AppState;
use ds_api::raw::request::message::{Message as AgentMessage, Role};

// How often (at most) we edit the placeholder while tokens stream in.
const STREAM_EDIT_INTERVAL: Duration = Duration::from_millis(800);

// ── Serenity boilerplate ──────────────────────────────────────────────────────

pub struct Handler {
    pub state: Arc<AppState>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _: Context, ready: Ready) {
        info!(user = %ready.user.tag(), "connected to Discord");
    }

    async fn message(&self, ctx: Context, msg: DiscordMessage) {
        // Ignore bots (including ourselves) and empty messages.
        if msg.author.bot || msg.content.trim().is_empty() {
            return;
        }

        let channel_id = msg.channel_id;
        let text = msg.content.clone();
        let author_name = msg.author.name.clone();

        info!(
            channel = %channel_id,
            author = %msg.author.tag(),
            "incoming message"
        );

        // Persist the incoming user message immediately.
        {
            let mut user_msg = AgentMessage::new(Role::User, &text);
            user_msg.name = Some(author_name.clone());
            self.state.persist_message(
                channel_id,
                &user_msg,
                self.state.db.clone(),
                self.state.embed.clone(),
            );
        }

        // Post a placeholder that we'll edit as the agent responds.
        let placeholder: DiscordMessage = match channel_id
            .send_message(&ctx.http, CreateMessage::new().content("…"))
            .await
        {
            Ok(m) => m,
            Err(e) => {
                error!("failed to send placeholder: {e}");
                return;
            }
        };

        tokio::spawn(handle_turn(
            Arc::clone(&ctx.http),
            Arc::clone(&self.state),
            channel_id,
            placeholder,
            text,
            author_name,
        ));
    }
}

/// Build and start the serenity `Client`. Returns when the gateway disconnects.
pub async fn run(discord_token: &str, state: Arc<AppState>) {
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    let mut client = Client::builder(discord_token, intents)
        .event_handler(Handler { state })
        .await
        .expect("failed to create Discord client");

    if let Err(e) = client.start().await {
        error!("Discord client error: {e}");
    }
}

// ── Agent turn ────────────────────────────────────────────────────────────────

async fn handle_turn(
    http: Arc<Http>,
    state: Arc<AppState>,
    channel_id: serenity::model::id::ChannelId,
    mut placeholder: serenity::model::channel::Message,
    text: String,
    author_name: String,
) {
    // Try to take the agent out of the shared map.
    // If it's busy, inject via interrupt channel instead.
    let agent_opt = state
        .with_chat_async(channel_id, |entry| {
            if entry.agent.is_some() {
                entry.agent.take()
            } else {
                let _ = entry.interrupt_tx.send(text.clone());
                None
            }
        })
        .await;

    let Some(mut agent) = agent_opt else {
        info!(channel = %channel_id, "agent busy — injected via interrupt channel");
        let _ = placeholder
            .edit(&http, EditMessage::new().content("*(queued)*"))
            .await;
        return;
    };

    // Push user message with name so the model knows who is speaking.
    agent.push_user_message_with_name(&text, Some(&author_name));

    let mut reply_buf = String::new();
    let mut last_edit = Instant::now();

    // Track per-tool embed messages so we can edit them when results arrive.
    // Key = tool call id, Value = the embed message.
    let mut tool_messages: std::collections::HashMap<String, serenity::model::channel::Message> =
        std::collections::HashMap::new();

    // Files to upload after the stream finishes.
    // Each entry: (tool_call_id, filename, bytes)
    let mut pending_uploads: Vec<(String, String, Vec<u8>)> = Vec::new();

    // Whether the placeholder has been committed (flushed with real content).
    let mut placeholder_used = false;

    let mut stream = agent.chat_from_history();

    while let Some(event) = stream.next().await {
        match event {
            // ── Streaming text ────────────────────────────────────────────────
            Ok(AgentEvent::Token(token)) => {
                reply_buf.push_str(&token);

                if last_edit.elapsed() >= STREAM_EDIT_INTERVAL {
                    let display = truncate_for_discord(&reply_buf);
                    let _ = placeholder
                        .edit(&http, EditMessage::new().content(display))
                        .await;
                    last_edit = Instant::now();
                }
            }

            // ── Tool call starting ────────────────────────────────────────────
            Ok(AgentEvent::ToolCall(info)) => {
                // Flush whatever text has accumulated before this tool call.
                if !reply_buf.is_empty() {
                    let display = truncate_for_discord(&reply_buf);
                    let _ = placeholder
                        .edit(&http, EditMessage::new().content(display))
                        .await;
                    reply_buf.clear();
                    placeholder_used = true;
                } else if !placeholder_used {
                    // No text yet — delete the bare "…" placeholder so the tool
                    // embed appears first without an empty message above it.
                    let _ = placeholder.delete(&http).await;
                    placeholder_used = true;
                }

                let args_str = info.args.to_string();
                let args_preview = if args_str.len() > 900 {
                    format!("{}…", &args_str[..900])
                } else {
                    args_str
                };

                let embed = CreateEmbed::new()
                    .title(format!("⚙️ {}", info.name))
                    .description(format!("```json\n{}\n```", args_preview))
                    .colour(0x5865F2);

                match channel_id
                    .send_message(&http, CreateMessage::new().embed(embed))
                    .await
                {
                    Ok(m) => {
                        tool_messages.insert(info.id.clone(), m);
                    }
                    Err(e) => error!("failed to send tool call embed: {e}"),
                }
            }

            // ── Tool result ───────────────────────────────────────────────────
            Ok(AgentEvent::ToolResult(res)) => {
                let wants_upload = res
                    .result
                    .get("upload")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                if wants_upload {
                    let filename = res
                        .result
                        .get("filename")
                        .and_then(|v| v.as_str())
                        .unwrap_or("file")
                        .to_string();
                    let data_b64 = res
                        .result
                        .get("data_base64")
                        .and_then(|v| v.as_str())
                        .unwrap_or_default();

                    use base64::{Engine, engine::general_purpose::STANDARD as BASE64};
                    match BASE64.decode(data_b64) {
                        Ok(bytes) => {
                            pending_uploads.push((res.id.clone(), filename, bytes));
                        }
                        Err(e) => error!("base64 decode error: {e}"),
                    }
                }

                if let Some(_tool_msg) = tool_messages.remove(&res.id) {
                    let has_error = res.result.get("error").is_some();
                    let colour = if has_error { 0xED4245 } else { 0x57F287 };
                    let icon = if has_error { "❌" } else { "✅" };

                    let description = if wants_upload {
                        let size = res.result.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
                        format!(
                            "📎 `{}` ({} bytes)",
                            res.result
                                .get("filename")
                                .and_then(|v| v.as_str())
                                .unwrap_or("file"),
                            size
                        )
                    } else {
                        let s = res.result.to_string();
                        let preview = if s.len() > 900 {
                            format!("{}…", &s[..900])
                        } else {
                            s
                        };
                        format!("```json\n{}\n```", preview)
                    };

                    let embed = CreateEmbed::new()
                        .title(format!("{icon} {}", res.name))
                        .description(description)
                        .colour(colour);

                    if let Err(e) = channel_id
                        .send_message(&http, CreateMessage::new().embed(embed))
                        .await
                    {
                        error!("failed to send tool result embed: {e}");
                    }

                    // Fresh placeholder for text that follows, now guaranteed to be
                    // after the result embed in the channel timeline.
                    match channel_id
                        .send_message(&http, CreateMessage::new().content("…"))
                        .await
                    {
                        Ok(m) => {
                            placeholder = m;
                            placeholder_used = false;
                            last_edit = Instant::now();
                        }
                        Err(e) => error!("failed to send post-result placeholder: {e}"),
                    }
                }
            }

            Err(e) => {
                error!(channel = %channel_id, "agent error: {e}");
                let _ = placeholder
                    .edit(&http, EditMessage::new().content(format!("⚠️ Error: {e}")))
                    .await;

                if let Some(recovered) = stream.into_agent() {
                    state
                        .with_chat_async(channel_id, |entry| entry.agent = Some(recovered))
                        .await;
                }
                return;
            }
        }
    }

    // Recover agent before any remaining I/O so follow-up messages can acquire it.
    if let Some(recovered) = stream.into_agent() {
        state
            .with_chat_async(channel_id, |entry| entry.agent = Some(recovered))
            .await;
    }

    // Final edit of the reply placeholder.
    if !reply_buf.is_empty() {
        let assistant_msg = AgentMessage::new(Role::Assistant, &reply_buf);
        state.persist_message(
            channel_id,
            &assistant_msg,
            state.db.clone(),
            state.embed.clone(),
        );

        let display = truncate_for_discord(&reply_buf);
        let _ = placeholder
            .edit(&http, EditMessage::new().content(display))
            .await;
    } else {
        let _ = placeholder.delete(&http).await;
    }

    // Upload any queued files.
    for (_id, filename, bytes) in pending_uploads {
        let attachment = CreateAttachment::bytes(bytes, filename);
        if let Err(e) = channel_id
            .send_message(&http, CreateMessage::new().add_file(attachment))
            .await
        {
            error!("failed to upload file: {e}");
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Discord's regular message limit is 2000 characters.
fn truncate_for_discord(s: &str) -> String {
    const LIMIT: usize = 1900;
    if s.len() <= LIMIT {
        s.to_string()
    } else {
        format!("{}…\n*(truncated)*", &s[..LIMIT])
    }
}
