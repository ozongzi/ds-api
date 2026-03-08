use std::collections::HashMap;

use crate::api::ApiClient;
use crate::conversation::{Conversation, Summarizer};
use crate::raw::request::message::{Message, Role};
use crate::tool_trait::Tool;
use serde_json::Value;

/// Information about a tool call requested by the model.
///
/// Yielded as `AgentEvent::ToolCall` when the model requests a tool invocation.
/// At this point the tool has not yet been executed.
#[derive(Debug, Clone)]
pub struct ToolCallInfo {
    pub id: String,
    pub name: String,
    pub args: Value,
}

/// The result of a completed tool invocation.
///
/// Yielded as `AgentEvent::ToolResult` after the tool has finished executing.
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub id: String,
    pub name: String,
    pub args: Value,
    pub result: Value,
}

/// Events emitted by [`AgentStream`][crate::agent::AgentStream].
///
/// Each variant represents a distinct, self-contained event in the agent lifecycle:
///
/// - `Token(String)` — a text fragment from the assistant.  In streaming mode each
///   `Token` is a single SSE delta; in non-streaming mode the full response text
///   arrives as one `Token`.
/// - `ToolCall(ToolCallInfo)` — the model has requested a tool invocation.  One event
///   is emitted per call, before execution begins.
/// - `ToolResult(ToolCallResult)` — a tool has finished executing.  One event is
///   emitted per call, in the same order as the corresponding `ToolCall` events.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    Token(String),
    ToolCall(ToolCallInfo),
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
}

impl DeepseekAgent {
    /// Create a new agent with the given API token.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            conversation: Conversation::new(ApiClient::new(token)),
            tools: vec![],
            tool_index: HashMap::new(),
            streaming: false,
        }
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

    /// Enable SSE streaming for each API turn (builder-style).
    pub fn with_streaming(mut self) -> Self {
        self.streaming = true;
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
}
