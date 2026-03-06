//! ApiRequest builder module.
//!
//! Provides a safe, chainable request builder that wraps the internal
//! `crate::raw::ChatCompletionRequest`.

use crate::raw::{ChatCompletionRequest, Message, ResponseFormat, ResponseFormatType, Tool};

/// A safe, chainable request builder that wraps `ChatCompletionRequest`.
///
/// It intentionally avoids exposing raw configuration enums (like `Model`) to
/// callers. Use the provided helpers to pick models.
#[derive(Debug)]
pub struct ApiRequest {
    raw: ChatCompletionRequest,
}

impl ApiRequest {
    /// Start a new builder with default values.
    pub fn builder() -> Self {
        Self {
            raw: ChatCompletionRequest::default(),
        }
    }

    pub fn model(mut self, model: crate::raw::Model) -> Self {
        self.raw.model = model;
        self
    }

    /// Convenience constructor: deepseek-chat + messages
    pub fn deepseek_chat(messages: Vec<Message>) -> Self {
        let mut r = Self::builder();
        r.raw.messages = messages;
        r.raw.model = crate::raw::Model::DeepseekChat;
        r
    }

    /// Convenience constructor: deepseek-reasoner + messages
    pub fn deepseek_reasoner(messages: Vec<Message>) -> Self {
        let mut r = Self::builder();
        r.raw.messages = messages;
        r.raw.model = crate::raw::Model::DeepseekReasoner;
        r
    }

    /// Add a message to the request.
    pub fn add_message(mut self, msg: Message) -> Self {
        self.raw.messages.push(msg);
        self
    }

    /// Replace messages.
    pub fn messages(mut self, msgs: Vec<Message>) -> Self {
        self.raw.messages = msgs;
        self
    }

    /// Request response as JSON object.
    pub fn json(mut self) -> Self {
        self.raw.response_format = Some(ResponseFormat {
            r#type: ResponseFormatType::JsonObject,
        });
        self
    }

    /// Request response as plain text.
    pub fn text(mut self) -> Self {
        self.raw.response_format = Some(ResponseFormat {
            r#type: ResponseFormatType::Text,
        });
        self
    }

    /// Set temperature.
    pub fn temperature(mut self, t: f32) -> Self {
        self.raw.temperature = Some(t);
        self
    }

    /// Set max tokens.
    pub fn max_tokens(mut self, n: u32) -> Self {
        self.raw.max_tokens = Some(n);
        self
    }

    /// Add a raw tool definition (from `crate::raw::Tool`).
    pub fn add_tool(mut self, tool: Tool) -> Self {
        if let Some(ref mut v) = self.raw.tools {
            v.push(tool);
        } else {
            self.raw.tools = Some(vec![tool]);
        }
        self
    }

    /// Set tool choice to Auto.
    pub fn tool_choice_auto(mut self) -> Self {
        use crate::raw::request::tool_choice::{ToolChoice, ToolChoiceType};
        self.raw.tool_choice = Some(ToolChoice::String(ToolChoiceType::Auto));
        self
    }

    /// Enable/disable streaming (stream: true).
    pub fn stream(mut self, enabled: bool) -> Self {
        self.raw.stream = Some(enabled);
        self
    }

    /// Build and return the internal raw request (crate-internal use).
    pub(crate) fn into_raw(self) -> ChatCompletionRequest {
        self.raw
    }
}
