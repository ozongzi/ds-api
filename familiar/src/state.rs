use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ds_api::DeepseekAgent;
use sqlx::PgPool;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, info};
use uuid::Uuid;

use crate::config::Config;
use crate::db::{Db, to_vector};
use crate::embedding::EmbeddingClient;
use crate::tools::{CommandTool, FileTool, HistoryTool, PresentFileTool, ScriptTool};

/// One entry per conversation.
pub struct ChatEntry {
    pub agent: Option<DeepseekAgent>,
    /// Kept alive so the agent's receiver is never dropped while the entry exists.
    pub interrupt_tx: UnboundedSender<String>,
}

/// Shared application state, held behind `Arc`.
pub struct AppState {
    /// Per-conversation agent instances, keyed by conversation UUID.
    /// Wrapped in `Arc` so the web layer can share this map without cloning the whole state.
    pub chats: Arc<Mutex<HashMap<Uuid, ChatEntry>>>,

    /// DeepSeek API key, used when constructing new agents.
    pub deepseek_token: String,

    /// Optional system prompt applied to every freshly created agent.
    pub system_prompt: Option<String>,

    /// PostgreSQL connection pool.
    pub pool: PgPool,

    /// Thin wrapper around the pool for message persistence.
    pub db: Db,

    /// Embedding client — shared across all conversations.
    pub embed: EmbeddingClient,
}

impl AppState {
    pub fn new(cfg: &Config, pool: PgPool) -> Self {
        let db = Db::new(pool.clone());
        Self {
            chats: Arc::new(Mutex::new(HashMap::new())),
            deepseek_token: cfg.deepseek_token.clone(),
            system_prompt: cfg.system_prompt.clone(),
            pool,
            db,
            embed: EmbeddingClient::new(cfg.openrouter_token.clone()),
        }
    }

    /// Build a fresh agent for `conversation_id`, restoring history from PG.
    pub async fn build_agent(
        &self,
        conversation_id: Uuid,
    ) -> (DeepseekAgent, UnboundedSender<String>) {
        let history = match self.db.restore(conversation_id).await {
            Ok(h) => {
                info!(conversation = %conversation_id, messages = h.len(), "restored history");
                h
            }
            Err(e) => {
                error!(conversation = %conversation_id, "failed to restore history: {e}");
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
                conversation_id,
            });

        if let Some(prompt) = &self.system_prompt {
            builder = builder.with_system_prompt(prompt.clone());
        }

        builder.with_interrupt_channel()
    }

    /// Get or create the `ChatEntry` for `conversation_id`.
    ///
    /// If the entry already exists, calls `f` immediately (sync, no DB hit).
    /// If it does not exist, builds a new agent first (async), then calls `f`.
    pub async fn with_chat_async<R>(
        &self,
        conversation_id: Uuid,
        f: impl FnOnce(&mut ChatEntry) -> R,
    ) -> R {
        // Fast path: entry already exists.
        {
            let mut map = self.chats.lock().unwrap();
            if let Some(entry) = map.get_mut(&conversation_id) {
                return f(entry);
            }
        }

        // Slow path: build agent (async, outside the lock).
        let (agent, tx) = self.build_agent(conversation_id).await;
        let mut entry = ChatEntry {
            agent: Some(agent),
            interrupt_tx: tx,
        };
        let result = f(&mut entry);
        self.chats.lock().unwrap().insert(conversation_id, entry);
        result
    }

    /// Append a message to the database and kick off embedding in the background.
    /// Fire-and-forget: errors are logged, never propagated.
    pub fn persist_message(
        &self,
        conversation_id: Uuid,
        msg: &ds_api::raw::request::message::Message,
    ) {
        let db = self.db.clone();
        let embed = self.embed.clone();
        let msg = msg.clone();

        tokio::spawn(async move {
            // Insert with no embedding first so the row exists immediately.
            let row_id = match db.append(conversation_id, &msg, None).await {
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
                                let vector = to_vector(vec);
                                if let Err(e) = db.set_embedding(row_id, vector).await {
                                    error!("set_embedding failed: {e}");
                                }
                            }
                            Err(e) => error!("embed failed: {e}"),
                        }
                    }
                }
            }
        });
    }
}
