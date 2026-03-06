use std::collections::HashMap;

use crate::api::ApiClient;
use crate::conversation::{Conversation, DeepseekConversation, Summarizer};
use crate::raw::request::message::{Message, Role};
use crate::tool_trait::Tool;
use serde_json::Value;

/// Tool call event (result).
///
/// Represents a single tool invocation result produced by the agent.
#[derive(Debug, Clone)]
pub struct ToolCallEvent {
    pub id: String,
    pub name: String,
    pub args: Value,
    pub result: Value,
}

/// Single agent response exposed to callers.
///
/// May contain assistant text content or a list of tool call events.
#[derive(Debug, Clone)]
pub struct AgentResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCallEvent>,
}

/// DeepseekAgent: encapsulates a conversation (`Conversation`) and a collection of tools.
///
/// Responsible for coordinating API calls and executing tools as requested by the model.
/// Fields use `pub(crate)` visibility so the sibling `stream` submodule can access internal
/// state (for example `tools` and `tool_index`).
pub struct DeepseekAgent {
    pub(crate) client: ApiClient,
    pub(crate) conversation: DeepseekConversation,
    pub(crate) tools: Vec<Box<dyn Tool>>,
    pub(crate) tool_index: HashMap<String, usize>,
}

impl DeepseekAgent {
    /// Create an Agent using the provided token.
    ///
    /// This internally constructs an `ApiClient` and attaches a `DeepseekConversation`.
    pub fn new(token: impl Into<String>) -> Self {
        let client = ApiClient::new(token.into());
        let conversation = DeepseekConversation::new(client.clone());
        Self {
            client,
            conversation,
            tools: vec![],
            tool_index: HashMap::new(),
        }
    }

    /// Add a tool (supports method chaining).
    ///
    /// Registers the tool and indexes its raw (protocol) function names so incoming
    /// tool call requests can be routed to the correct implementation.
    pub fn add_tool<TT: Tool + 'static>(mut self, tool: TT) -> Self {
        let idx = self.tools.len();
        for raw in tool.raw_tools() {
            // raw.function.name is the protocol-level name used to match tool call requests
            self.tool_index.insert(raw.function.name.clone(), idx);
        }
        self.tools.push(Box::new(tool));
        self
    }

    /// Push a user message into the conversation and return an `AgentStream`.
    ///
    /// The returned `AgentStream` drives the API request and any subsequent tool execution.
    /// The return type uses a fully-qualified path and depends on the sibling `stream`
    /// submodule providing `AgentStream`.
    pub fn chat(mut self, user_message: &str) -> crate::agent::stream::AgentStream {
        self.conversation.push_user_input(user_message.to_string());
        crate::agent::stream::AgentStream::new(self)
    }

    /// Set a custom system prompt to inject at the start of the conversation.
    ///
    /// Builder-style: returns `self` so the call can be chained.
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        let p = prompt.into();
        // Add a system message to the conversation history
        self.conversation
            .add_message(Message::new(Role::System, p.as_str()));
        self
    }

    /// Set a summarizer to use for conversation summarization.
    ///
    /// Builder-style: returns `self` so the call can be chained.
    pub fn with_summarizer(mut self, summarizer: impl Summarizer + 'static) -> Self {
        self.conversation = self.conversation.with_summarizer(summarizer);
        self
    }
}
