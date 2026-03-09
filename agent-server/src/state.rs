use std::collections::HashMap;
use std::sync::Mutex;

use ds_api::DeepseekAgent;
use tokio::sync::mpsc::UnboundedSender;

use crate::config::Config;

/// One entry per Telegram chat ID.
///
/// The agent is stored as `Option` so we can `take()` it out while a turn is
/// running (the agent is move-consumed by `chat()`) and `put()` it back when
/// the stream finishes.
pub struct ChatEntry {
    pub agent: Option<DeepseekAgent>,
    /// Sender half of the interrupt channel — kept alive so the agent's
    /// receiver is never dropped while the entry exists.
    pub interrupt_tx: UnboundedSender<String>,
}

/// Shared application state, held behind `Arc` in every axum handler.
pub struct AppState {
    /// Per-chat agent instances, keyed by Telegram chat ID.
    pub chats: Mutex<HashMap<i64, ChatEntry>>,

    /// Telegram Bot API token, used by the bot module to call sendMessage etc.
    pub telegram_token: String,

    /// DeepSeek API key, used when constructing new agents.
    pub deepseek_token: String,

    /// Optional system prompt applied to every freshly created agent.
    pub system_prompt: Option<String>,

    /// Optional webhook secret for request verification.
    pub webhook_secret: Option<String>,
}

impl AppState {
    pub fn new(cfg: &Config) -> Self {
        Self {
            chats: Mutex::new(HashMap::new()),
            telegram_token: cfg.telegram_token.clone(),
            deepseek_token: cfg.deepseek_token.clone(),
            system_prompt: cfg.system_prompt.clone(),
            webhook_secret: cfg.webhook_secret.clone(),
        }
    }

    /// Return a fresh `DeepseekAgent` configured with this server's credentials
    /// and system prompt.  The interrupt channel is wired up and the sender is
    /// returned alongside the agent so the caller can store it in `ChatEntry`.
    pub fn build_agent(&self) -> (DeepseekAgent, UnboundedSender<String>) {
        let mut builder = DeepseekAgent::new(self.deepseek_token.clone()).with_streaming();

        if let Some(prompt) = &self.system_prompt {
            builder = builder.with_system_prompt(prompt.clone());
        }

        builder.with_interrupt_channel()
    }

    /// Get or create the `ChatEntry` for `chat_id`.
    ///
    /// If the entry does not exist yet, a new agent (+ interrupt channel) is
    /// constructed and stored.  The closure `f` is called with a mutable
    /// reference to the entry while the lock is held.
    pub fn with_chat<R>(&self, chat_id: i64, f: impl FnOnce(&mut ChatEntry) -> R) -> R {
        let mut map = self.chats.lock().unwrap();
        let entry = map.entry(chat_id).or_insert_with(|| {
            let (agent, tx) = self.build_agent();
            ChatEntry {
                agent: Some(agent),
                interrupt_tx: tx,
            }
        });
        f(entry)
    }
}
