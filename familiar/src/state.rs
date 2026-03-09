use std::collections::HashMap;
use std::sync::Mutex;

use ds_api::DeepseekAgent;
use serenity::model::id::ChannelId;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, info};

use crate::config::Config;
use crate::db::{Db, encode_embedding};
use crate::embedding::EmbeddingClient;
use crate::tools::{CommandTool, FileTool, HistoryTool, PresentFileTool, ScriptTool};

/// One entry per Discord channel.
pub struct ChatEntry {
    pub agent: Option<DeepseekAgent>,
    /// Kept alive so the agent's receiver is never dropped while the entry exists.
    pub interrupt_tx: UnboundedSender<String>,
}

/// Shared application state, held behind `Arc` in the serenity event handler.
pub struct AppState {
    /// Per-channel agent instances, keyed by Discord ChannelId.
    pub chats: Mutex<HashMap<ChannelId, ChatEntry>>,

    /// DeepSeek API key, used when constructing new agents.
    pub deepseek_token: String,

    /// Optional system prompt applied to every freshly created agent.
    pub system_prompt: Option<String>,

    /// SQLite database handle — shared across all channels.
    pub db: Db,

    /// Embedding client — shared across all channels.
    pub embed: EmbeddingClient,
}

impl AppState {
    pub fn new(cfg: &Config, db: Db) -> Self {
        Self {
            chats: Mutex::new(HashMap::new()),
            deepseek_token: cfg.deepseek_token.clone(),
            system_prompt: cfg.system_prompt.clone(),
            db,
            embed: EmbeddingClient::new(cfg.openrouter_token.clone()),
        }
    }

    /// Build a fresh agent for `channel_id`, restoring history from SQLite.
    /// This is async because it queries the database.
    pub async fn build_agent(
        &self,
        channel_id: ChannelId,
    ) -> (DeepseekAgent, UnboundedSender<String>) {
        let channel_str = channel_id.to_string();

        // Restore history from SQLite.
        let history = match self.db.restore(&channel_str).await {
            Ok(h) => {
                info!(channel = %channel_id, messages = h.len() as u64, "restored history");
                h
            }
            Err(e) => {
                error!(channel = %channel_id, "failed to restore history: {e}");
                vec![]
            }
        };

        let mut builder = DeepseekAgent::new(self.deepseek_token.clone())
            .with_streaming()
            .with_history(history)
            .add_tool(CommandTool)
            .add_tool(FileTool)
            .add_tool(ScriptTool)
            .add_tool(PresentFileTool)
            .add_tool(HistoryTool {
                db: self.db.clone(),
                embed: self.embed.clone(),
                channel_id: channel_str,
            });

        if let Some(prompt) = &self.system_prompt {
            builder = builder.with_system_prompt(prompt.clone());
        }

        builder.with_interrupt_channel()
    }

    /// Get or create the `ChatEntry` for `channel_id`.
    ///
    /// If the entry already exists, calls `f` immediately (sync, no DB hit).
    /// If it does not exist, builds a new agent first (async), then calls `f`.
    pub async fn with_chat_async<R>(
        &self,
        channel_id: ChannelId,
        f: impl FnOnce(&mut ChatEntry) -> R,
    ) -> R {
        // Fast path: entry already exists.
        {
            let mut map = self.chats.lock().unwrap();
            if let Some(entry) = map.get_mut(&channel_id) {
                return f(entry);
            }
        }

        // Slow path: build agent (async, outside the lock).
        let (agent, tx) = self.build_agent(channel_id).await;
        let mut entry = ChatEntry {
            agent: Some(agent),
            interrupt_tx: tx,
        };
        let result = f(&mut entry);
        self.chats.lock().unwrap().insert(channel_id, entry);
        result
    }

    /// Append a message to the database and kick off embedding in the background.
    ///
    /// Fire-and-forget: errors are logged, never propagated.
    pub fn persist_message(
        &self,
        channel_id: ChannelId,
        msg: &ds_api::raw::request::message::Message,
        db: Db,
        embed: EmbeddingClient,
    ) {
        let channel_str = channel_id.to_string();
        let msg = msg.clone();

        tokio::spawn(async move {
            // Insert with empty embedding first so the row exists immediately.
            let row_id = match db.append(&channel_str, &msg, vec![]).await {
                Ok(id) => id,
                Err(e) => {
                    error!("db append failed: {e}");
                    return;
                }
            };

            // Only embed user and assistant text content.
            let should_embed = matches!(
                msg.role,
                ds_api::raw::request::message::Role::User
                    | ds_api::raw::request::message::Role::Assistant
            );

            if should_embed {
                if let Some(text) = &msg.content {
                    if !text.is_empty() {
                        match embed.embed(text).await {
                            Ok(vec) => {
                                let blob = encode_embedding(vec.as_slice());
                                if let Err(e) = db.set_embedding(row_id, blob).await {
                                    error!("set_embedding failed: {e}");
                                }
                            }
                            Err(e) => error!("embed failed: {}", e),
                        }
                    }
                }
            }
        });
    }
}
