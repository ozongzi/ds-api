use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ds_api::AgentEvent;
use ds_api::DeepseekAgent;
use ds_api::McpTool;
use sqlx::PgPool;
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::config::Config;
use crate::db::{Db, to_vector};
use crate::embedding::EmbeddingClient;
use crate::tools::{A2aTool, CommandTool, FileTool, HistoryTool, PresentFileTool, ScriptTool};
use std::sync::atomic::{AtomicBool, Ordering};

// How many events to keep in the log for late-joining clients.
// Enough to replay a full long turn including many tool calls.
const EVENT_LOG_CAP: usize = 4096;

// broadcast channel capacity — how many events can be buffered before
// a slow subscriber starts missing messages.
const BROADCAST_CAP: usize = 256;

/// A single event that was (or will be) sent over WebSocket.
/// Stored in the event log so late-joining clients can replay.
#[derive(Debug, Clone)]
pub struct WsEvent {
    pub payload: String, // serialised JSON, ready to send
}

/// One entry per conversation.
pub struct ChatEntry {
    /// The agent when idle (not generating). Taken out during generation.
    pub agent: Option<DeepseekAgent>,

    /// Kept alive so the agent's interrupt receiver is never dropped.
    pub interrupt_tx: UnboundedSender<String>,

    /// Broadcast sender — the background generation task sends every event here.
    /// WebSocket handlers subscribe to receive live events.
    pub broadcast_tx: broadcast::Sender<Arc<WsEvent>>,

    /// Ordered log of every event emitted in the current (or most recent) turn.
    /// New WebSocket clients replay this before subscribing to live events so
    /// they catch up even if they connected mid-generation or after it finished.
    pub event_log: Vec<Arc<WsEvent>>,

    /// True while a background generation task is running for this conversation.
    pub generating: bool,

    /// Set to true by ws.rs when the client sends { type: "abort" }.
    /// The generation task polls this flag and stops early when it's set.
    pub abort_flag: Arc<AtomicBool>,
}

impl ChatEntry {
    fn new(agent: DeepseekAgent, interrupt_tx: UnboundedSender<String>) -> Self {
        let (broadcast_tx, _) = broadcast::channel(BROADCAST_CAP);
        Self {
            agent: Some(agent),
            interrupt_tx,
            broadcast_tx,
            event_log: Vec::new(),
            generating: false,
            abort_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Emit an event: append to the log and broadcast to all live subscribers.
    /// The log is capped at EVENT_LOG_CAP to avoid unbounded memory growth.
    pub fn emit(&mut self, payload: String) {
        let ev = Arc::new(WsEvent { payload });
        if self.event_log.len() >= EVENT_LOG_CAP {
            self.event_log.remove(0);
        }
        self.event_log.push(Arc::clone(&ev));
        // Ignore send errors — no subscribers is fine.
        let _ = self.broadcast_tx.send(ev);
    }

    /// Clear the event log for a new generation turn.
    pub fn clear_log(&mut self) {
        self.event_log.clear();
    }
}

/// Shared application state, held behind `Arc`.
#[derive(Clone)]
pub struct AppState {
    /// Per-conversation agent instances, keyed by conversation UUID.
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

    /// MCP tools initialised once at startup and shared across all agents.
    /// Each agent gets a cheap clone (all internals are Arc).
    pub mcp_tools: Vec<McpTool>,
}

impl AppState {
    pub fn new(cfg: &Config, pool: PgPool, mcp_tools: Vec<McpTool>) -> Self {
        let db = Db::new(pool.clone());
        Self {
            chats: Arc::new(Mutex::new(HashMap::new())),
            deepseek_token: cfg.deepseek_token.clone(),
            system_prompt: cfg.system_prompt.clone(),
            pool,
            db,
            embed: EmbeddingClient::new(cfg.openrouter_token.clone()),
            mcp_tools,
        }
    }

    /// Initialise MCP servers. Called once at startup.
    /// Failures are logged and skipped — a missing MCP server should never
    /// prevent familiar from starting.
    pub async fn init_mcp() -> Vec<McpTool> {
        let mut tools = Vec::new();

        match McpTool::stdio("npx", &["-y", "@playwright/mcp"]).await {
            Ok(t) => {
                info!("MCP: playwright ready");
                tools.push(t);
            }
            Err(e) => warn!("MCP: playwright failed to start: {e}"),
        }
        match McpTool::stdio("uvx", &["mcp-server-fetch"]).await {
            Ok(t) => {
                info!("MCP: fetch ready");
                tools.push(t);
            }
            Err(e) => warn!("MCP: fetch failed to start: {e}"),
        }
        match McpTool::stdio("npx", &["-y", "@modelcontextprotocol/server-github"]).await {
            Ok(t) => {
                info!("MCP: github ready");
                tools.push(t);
            }
            Err(e) => warn!("MCP: github failed to start: {e}"),
        }

        tools
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

        let mut builder = DeepseekAgent::custom(
            self.deepseek_token.clone(),
            "https://api.deepseek.com",
            "deepseek-reasoner",
        )
        .with_streaming()
        .with_history(history)
        .add_tool(CommandTool)
        .add_tool(FileTool)
        .add_tool(ScriptTool)
        .add_tool(PresentFileTool)
        .add_tool(A2aTool)
        .add_tool(HistoryTool {
            db: self.db.clone(),
            embed: self.embed.clone(),
            conversation_id,
        });

        for mcp_tool in &self.mcp_tools {
            builder = builder.add_tool(mcp_tool.clone());
        }

        if let Some(prompt) = &self.system_prompt {
            builder = builder.with_system_prompt(prompt.clone());
        }

        builder.with_interrupt_channel()
    }

    /// Ensure a `ChatEntry` exists for `conversation_id`, building one if needed.
    /// Returns a broadcast receiver (for live events) and the full event log
    /// snapshot (for replay), plus whether generation is currently in progress.
    pub async fn attach(
        &self,
        conversation_id: Uuid,
    ) -> (broadcast::Receiver<Arc<WsEvent>>, Vec<Arc<WsEvent>>, bool) {
        // Fast path — entry already exists.
        {
            let map = self.chats.lock().unwrap();
            if let Some(entry) = map.get(&conversation_id) {
                let rx = entry.broadcast_tx.subscribe();
                let log = entry.event_log.clone();
                let generating = entry.generating;
                return (rx, log, generating);
            }
        }

        // Slow path — build agent outside the lock.
        let (agent, tx) = self.build_agent(conversation_id).await;
        let entry = ChatEntry::new(agent, tx);
        let rx = entry.broadcast_tx.subscribe();
        let log = entry.event_log.clone();
        let generating = entry.generating;
        self.chats.lock().unwrap().insert(conversation_id, entry);
        (rx, log, generating)
    }

    /// Start a background generation task for `conversation_id`.
    ///
    /// Pushes `user_text` onto the agent, marks the entry as `generating`,
    /// clears the event log, and spawns a task that drives the agent stream,
    /// emitting every event through the broadcast channel and the log.
    ///
    /// Returns `false` if generation is already in progress (caller should
    /// send the event log replay + subscribe instead of starting a new turn).
    pub async fn start_generation(&self, conversation_id: Uuid, user_text: String) -> bool {
        // Take the agent out of the entry (if idle).
        let (agent, abort_flag) = {
            let mut map = self.chats.lock().unwrap();
            let entry = match map.get_mut(&conversation_id) {
                Some(e) => e,
                None => return false,
            };
            if entry.generating {
                return false;
            }
            // Clear previous turn's log and reset abort flag.
            entry.clear_log();
            entry.abort_flag.store(false, Ordering::Relaxed);
            entry.generating = true;
            (entry.agent.take(), Arc::clone(&entry.abort_flag))
        };

        let Some(mut agent) = agent else {
            // Agent missing (shouldn't happen after attach, but be safe).
            let mut map = self.chats.lock().unwrap();
            if let Some(entry) = map.get_mut(&conversation_id) {
                entry.generating = false;
            }
            return false;
        };

        agent.push_user_message_with_name(&user_text, None);

        let state = self.clone();

        tokio::spawn(async move {
            run_generation(state, conversation_id, agent, abort_flag).await;
        });

        true
    }

    /// Inject a message into a running generation via the interrupt channel.
    pub fn send_interrupt(&self, conversation_id: Uuid, content: String) {
        let map = self.chats.lock().unwrap();
        if let Some(entry) = map.get(&conversation_id) {
            let _ = entry.interrupt_tx.send(content);
        }
    }

    /// Signal the running generation task to stop as soon as possible.
    /// The task will emit an { type: "aborted" } event, persist what it
    /// has so far, recover the agent, and return.
    pub fn abort_generation(&self, conversation_id: Uuid) {
        let map = self.chats.lock().unwrap();
        if let Some(entry) = map.get(&conversation_id) {
            entry.abort_flag.store(true, Ordering::Relaxed);
        }
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
            let row_id = match db.append(conversation_id, &msg, None).await {
                Ok(id) => id,
                Err(e) => {
                    error!("db append failed: {e}");
                    return;
                }
            };

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

// ── Background generation task ────────────────────────────────────────────────

/// Drives the agent stream to completion, emitting every event through the
/// broadcast channel and the event log so any number of WebSocket clients
/// can subscribe (or catch up after reconnecting).
async fn run_generation(
    state: AppState,
    conversation_id: Uuid,
    agent: DeepseekAgent,
    abort_flag: Arc<AtomicBool>,
) {
    use ds_api::raw::request::message::{Message as AgentMessage, Role};
    use futures::StreamExt;
    use serde_json::json;

    let mut stream = agent.chat_from_history();
    let mut reply_buf = String::new();

    loop {
        // Check abort flag before polling the next event.
        if abort_flag.load(Ordering::Relaxed) {
            // Emit aborted event, persist what we have, recover agent.
            {
                let mut map = state.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conversation_id) {
                    entry.emit(json!({"type": "aborted"}).to_string());
                }
            }
            if !reply_buf.is_empty() {
                let msg = AgentMessage::new(Role::Assistant, &reply_buf);
                state.persist_message(conversation_id, &msg);
            }
            if let Some(recovered) = stream.into_agent() {
                let mut map = state.chats.lock().unwrap();
                if let Some(entry) = map.get_mut(&conversation_id) {
                    entry.agent = Some(recovered);
                    entry.generating = false;
                    entry.abort_flag.store(false, Ordering::Relaxed);
                }
            }
            return;
        }

        tokio::select! {
            biased;

            agent_event = stream.next() => {
                let Some(event) = agent_event else {
                    break;
                };

                let payload = match event {
                    Ok(AgentEvent::Token(token)) => {
                        reply_buf.push_str(&token);
                        json!({"type": "token", "content": token}).to_string()
                    }
                    Ok(AgentEvent::ToolCallStart { id, name }) => json!({
                        "type": "tool_call_start",
                        "id": id,
                        "name": name,
                    }).to_string(),
                    Ok(AgentEvent::ToolCallArgsDelta { id, delta }) => json!({
                        "type": "tool_call_args_delta",
                        "id": id,
                        "delta": delta,
                    }).to_string(),
                    Ok(AgentEvent::ToolCall(info)) => json!({
                        "type": "tool_call",
                        "id": info.id,
                        "name": info.name,
                        "args": info.args,
                    }).to_string(),
                    Ok(AgentEvent::ToolResult(res)) => json!({
                        "type": "tool_result",
                        "id": res.id,
                        "name": res.name,
                        "result": res.result,
                    }).to_string(),
                    Ok(AgentEvent::ReasoningToken(token)) => {
                        json!({"type": "reasoning_token", "content": token}).to_string()
                    }
                    Err(e) => {
                        error!(conversation = %conversation_id, "agent error: {e}");
                        let payload = json!({"type": "error", "message": e.to_string()}).to_string();
                        // Emit the error event before recovering.
                        {
                            let mut map = state.chats.lock().unwrap();
                            if let Some(entry) = map.get_mut(&conversation_id) {
                                entry.emit(payload);
                            }
                        }
                        // Recover the agent.
                        if let Some(recovered) = stream.into_agent() {
                            let mut map = state.chats.lock().unwrap();
                            if let Some(entry) = map.get_mut(&conversation_id) {
                                entry.agent = Some(recovered);
                                entry.generating = false;
                            }
                        }
                        return;
                    }
                };

                // Emit to log + broadcast.
                {
                    let mut map = state.chats.lock().unwrap();
                    if let Some(entry) = map.get_mut(&conversation_id) {
                        entry.emit(payload);
                    }
                }
            }

            else => break,
        }
    }

    // ── Recover agent ─────────────────────────────────────────────────────────
    let recovered = stream.into_agent();

    // ── Persist assistant reply ───────────────────────────────────────────────
    if !reply_buf.is_empty() {
        let msg = AgentMessage::new(Role::Assistant, &reply_buf);
        state.persist_message(conversation_id, &msg);
    }

    // ── Emit done, put agent back ─────────────────────────────────────────────
    {
        let mut map = state.chats.lock().unwrap();
        if let Some(entry) = map.get_mut(&conversation_id) {
            entry.emit(json!({"type": "done"}).to_string());
            entry.generating = false;
            if let Some(agent) = recovered {
                entry.agent = Some(agent);
            }
        }
    }
}
