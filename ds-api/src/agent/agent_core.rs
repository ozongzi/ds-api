use std::collections::HashMap;

use crate::api::ApiClient;
use crate::conversation::{Conversation, LlmSummarizer, Summarizer};
use crate::raw::request::message::{Message, Role};
use crate::tool_trait::Tool;
use serde_json::Value;
use tokio::sync::mpsc;

/// A tool call fragment emitted by [`AgentStream`][crate::agent::AgentStream].
///
/// In streaming mode multiple `ToolCallChunk`s are emitted per tool call:
/// the first has an empty `delta` (name is known, no args yet); subsequent
/// chunks carry incremental argument JSON.  In non-streaming mode a single
/// chunk is emitted with the complete argument JSON in `delta`.
#[derive(Debug, Clone)]
pub struct ToolCallChunk {
    pub id: String,
    pub name: String,
    pub delta: String,
}

/// The result of a completed tool invocation.
///
/// Yielded as `AgentEvent::ToolResult` after the tool has finished executing.
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub id: String,
    pub name: String,
    pub args: String,
    pub result: Value,
}

/// Events emitted by [`AgentStream`][crate::agent::AgentStream].
///
/// Each variant represents a distinct, self-contained event in the agent lifecycle:
///
/// - `Token(String)` — a text fragment from the assistant.  In streaming mode each
///   `Token` is a single SSE delta; in non-streaming mode the full response text
///   arrives as one `Token`.
/// - `ToolCall(id, name, delta)` — a tool call fragment.  Behaves exactly like
///   `Token`: in streaming mode one event is emitted per SSE chunk (first chunk has
///   an empty `delta` and carries the tool name; subsequent chunks carry incremental
///   argument JSON).  In non-streaming mode a single event is emitted with the
///   complete arguments string.  Accumulate `delta` values by `id` to reconstruct
///   the full argument JSON.  Execution begins after all chunks for a turn are
///   delivered.
/// - `ToolResult(ToolCallResult)` — a tool has finished executing.  One event is
///   emitted per call, in the same order as the corresponding `ToolCall` events.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Token(String),
    /// Emitted when the model produces reasoning/thinking content (e.g. deepseek-reasoner).
    /// In streaming mode this arrives token-by-token before the main reply.
    ReasoningToken(String),
    ToolCall(ToolCallChunk),
    ToolResult(ToolCallResult),
}

/// An agent that combines a [`Conversation`] with a set of callable tools.
///
/// Build one with the fluent builder methods, then call [`chat`][DeepseekAgent::chat]
/// to start a turn:
///
/// ```no_run
/// use ds_api::{DeepseekAgent, tool};
/// use serde_json::{Value, json};
///
/// struct MyTool;
///
/// #[tool]
/// impl ds_api::Tool for MyTool {
///     async fn greet(&self, name: String) -> Value {
///         json!({ "greeting": format!("Hello, {name}!") })
///     }
/// }
///
/// # #[tokio::main] async fn main() {
/// let agent = DeepseekAgent::new("sk-...")
///     .add_tool(MyTool);
/// # }
/// ```
pub struct DeepseekAgent {
    /// The conversation manages history, the API client, and context-window compression.
    pub(crate) conversation: Conversation,
    pub(crate) tools: Vec<Box<dyn Tool>>,
    pub(crate) tool_index: HashMap<String, usize>,
    /// When `true` the agent uses SSE streaming for each API turn so `Token` events
    /// arrive incrementally.  When `false` (default) the full response is awaited.
    pub(crate) streaming: bool,
    /// The model to use for every API turn.  Defaults to `"deepseek-chat"`.
    pub(crate) model: String,
    /// Optional channel for injecting user messages mid-loop.
    /// Messages received here are drained after each tool-execution round and
    /// appended to the conversation history as `Role::User` messages before the
    /// next API turn begins.
    pub(crate) interrupt_rx: Option<mpsc::UnboundedReceiver<String>>,
    /// Optional map of extra top-level JSON fields to merge into the API request body.
    /// This is used by the builder helpers below to attach custom provider-specific
    /// fields that the typed request doesn't yet expose.
    pub(crate) extra_body: Option<serde_json::Map<String, serde_json::Value>>,
}

impl DeepseekAgent {
    fn from_parts(client: ApiClient, model: impl Into<String>) -> Self {
        let model = model.into();
        let summarizer = LlmSummarizer::new(client.clone()).with_model(model.clone());
        Self {
            conversation: Conversation::new(client).with_summarizer(summarizer),
            tools: vec![],
            tool_index: HashMap::new(),
            streaming: false,
            model,
            interrupt_rx: None,
            extra_body: None,
        }
    }

    /// Create a new agent targeting the DeepSeek API with `deepseek-chat`.
    pub fn new(token: impl Into<String>) -> Self {
        Self::from_parts(ApiClient::new(token), "deepseek-chat")
    }

    /// Create an agent targeting an OpenAI-compatible provider.
    ///
    /// All three parameters are set at construction time and never change:
    ///
    /// ```no_run
    /// use ds_api::DeepseekAgent;
    ///
    /// let agent = DeepseekAgent::custom(
    ///     "sk-or-...",
    ///     "https://openrouter.ai/api/v1",
    ///     "meta-llama/llama-3.3-70b-instruct:free",
    /// );
    /// ```
    pub fn custom(
        token: impl Into<String>,
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        let client = ApiClient::new(token).with_base_url(base_url);
        Self::from_parts(client, model)
    }

    /// Register a tool (builder-style, supports chaining).
    ///
    /// The tool's protocol-level function names are indexed so incoming tool-call
    /// requests from the model can be dispatched to the correct implementation.
    pub fn add_tool<TT: Tool + 'static>(mut self, tool: TT) -> Self {
        let idx = self.tools.len();
        for raw in tool.raw_tools() {
            self.tool_index.insert(raw.function.name.clone(), idx);
        }
        self.tools.push(Box::new(tool));
        self
    }

    /// Push a user message and return an [`AgentStream`][crate::agent::AgentStream]
    /// that drives the full agent loop (API calls + tool execution).
    pub fn chat(mut self, user_message: &str) -> crate::agent::stream::AgentStream {
        self.conversation.push_user_input(user_message);
        crate::agent::stream::AgentStream::new(self)
    }

    /// Start an agent turn from the current history **without** pushing a new
    /// user message first.
    ///
    /// Use this when you have already appended the user message manually (e.g.
    /// via [`push_user_message_with_name`][Self::push_user_message_with_name])
    /// and want to drive the agent loop from that point.
    pub fn chat_from_history(self) -> crate::agent::stream::AgentStream {
        crate::agent::stream::AgentStream::new(self)
    }

    /// Enable SSE streaming for each API turn (builder-style).
    pub fn with_streaming(mut self) -> Self {
        self.streaming = true;
        self
    }

    /// Merge arbitrary top-level JSON key/value pairs into the request body for
    /// the next API turn. The pairs are stored on the agent and later merged
    /// into the `ApiRequest` raw body when a request is built.
    ///
    /// Example:
    /// let mut map = serde_json::Map::new();
    /// map.insert(\"foo\".to_string(), serde_json::json!(\"bar\"));
    /// let agent = DeepseekAgent::new(\"sk-...\").extra_body(map);
    pub fn extra_body(mut self, map: serde_json::Map<String, serde_json::Value>) -> Self {
        if let Some(ref mut existing) = self.extra_body {
            existing.extend(map);
        } else {
            self.extra_body = Some(map);
        }
        self
    }

    /// Add a single extra top-level field to be merged into the request body.
    /// Convenience helper to avoid constructing a full map.
    pub fn extra_field(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        if let Some(ref mut m) = self.extra_body {
            m.insert(key.into(), value);
        } else {
            let mut m = serde_json::Map::new();
            m.insert(key.into(), value);
            self.extra_body = Some(m);
        }
        self
    }

    /// Prepend a permanent system prompt to the conversation history (builder-style).
    ///
    /// System messages added this way are never removed by the built-in summarizers.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.conversation
            .add_message(Message::new(Role::System, &prompt.into()));
        self
    }

    /// Replace the summarizer used for context-window management (builder-style).
    pub fn with_summarizer(mut self, summarizer: impl Summarizer + 'static) -> Self {
        self.conversation = self.conversation.with_summarizer(summarizer);
        self
    }

    /// Seed the agent with an existing message history (builder-style).
    ///
    /// Used to restore a conversation from persistent storage (e.g. SQLite)
    /// after a process restart.  The messages are set directly on the
    /// underlying `Conversation` and will be included in the next API call.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ds_api::DeepseekAgent;
    /// use ds_api::raw::request::message::{Message, Role};
    ///
    /// # #[tokio::main] async fn main() {
    /// let history = vec![
    ///     Message::new(Role::User, "Hello"),
    ///     Message::new(Role::Assistant, "Hi there!"),
    /// ];
    /// let agent = DeepseekAgent::new("sk-...").with_history(history);
    /// # }
    /// ```
    pub fn with_history(mut self, history: Vec<crate::raw::request::message::Message>) -> Self {
        self.conversation = self.conversation.with_history(history);
        self
    }

    /// Append a user message with an optional display name to the conversation
    /// history.
    ///
    /// The `name` field is passed through to the API as-is (OpenAI-compatible
    /// providers use it to distinguish speakers in a shared channel).
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ds_api::DeepseekAgent;
    ///
    /// # #[tokio::main] async fn main() {
    /// let mut agent = DeepseekAgent::new("sk-...");
    /// agent.push_user_message_with_name("What time is it?", Some("alice"));
    /// # }
    /// ```
    pub fn push_user_message_with_name(&mut self, text: &str, name: Option<&str>) {
        use crate::raw::request::message::{Message, Role};
        let mut msg = Message::new(Role::User, text);
        msg.name = name.map(|n| n.to_string());
        self.conversation.history_mut().push(msg);
    }

    /// Read-only view of the current conversation history.
    ///
    /// Returns all messages in order, including system prompts, user turns,
    /// assistant replies, tool calls, and tool results.  Auto-summary messages
    /// inserted by the built-in summarizers are also included.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ds_api::DeepseekAgent;
    ///
    /// # #[tokio::main] async fn main() {
    /// let agent = DeepseekAgent::new("sk-...");
    /// for msg in agent.history() {
    ///     println!("{:?}: {:?}", msg.role, msg.content);
    /// }
    /// # }
    /// ```
    pub fn history(&self) -> &[crate::raw::request::message::Message] {
        self.conversation.history()
    }

    /// Attach an interrupt channel to the agent (builder-style).
    ///
    /// Returns the agent and the sender half of the channel.  Send any `String`
    /// through the `UnboundedSender` at any time; the message will be picked up
    /// after the current tool-execution round finishes and inserted into the
    /// conversation history as a `Role::User` message before the next API turn.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ds_api::DeepseekAgent;
    /// use tokio::sync::mpsc;
    ///
    /// # #[tokio::main] async fn main() {
    /// let (agent, tx) = DeepseekAgent::new("sk-...")
    ///     .with_interrupt_channel();
    ///
    /// // In another task or callback:
    /// tx.send("Actually, use Python instead.".into()).unwrap();
    /// # }
    /// ```
    pub fn with_interrupt_channel(mut self) -> (Self, mpsc::UnboundedSender<String>) {
        let (tx, rx) = mpsc::unbounded_channel();
        self.interrupt_rx = Some(rx);
        (self, tx)
    }

    /// Drain any pending messages from the interrupt channel and append them
    /// to the conversation history as `Role::User` messages.
    ///
    /// Called by the state machine in [`AgentStream`] at the top of every
    /// `Idle` transition so that injected messages are visible before each API
    /// turn, not just after tool-execution rounds.
    pub(crate) fn drain_interrupts(&mut self) {
        if let Some(rx) = self.interrupt_rx.as_mut() {
            while let Ok(msg) = rx.try_recv() {
                self.conversation
                    .history_mut()
                    .push(Message::new(Role::User, &msg));
            }
        }
    }
}
