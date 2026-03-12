//! ApiRequest builder module.
//!
//! Provides a safe, chainable request builder that wraps the internal
//! `crate::raw::ChatCompletionRequest`.

use crate::raw::{ChatCompletionRequest, Message, ResponseFormat, ResponseFormatType, Tool};

/// A safe, chainable request builder that wraps `ChatCompletionRequest`.
///
/// Use [`with_model`][ApiRequest::with_model] to set an arbitrary model string,
/// or the convenience constructors [`deepseek_chat`][ApiRequest::deepseek_chat]
/// and [`deepseek_reasoner`][ApiRequest::deepseek_reasoner] for the standard
/// DeepSeek models.
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

    /// Set the model by string (builder-style).
    ///
    /// Accepts any model identifier — a named DeepSeek model or any
    /// OpenAI-compatible model string:
    ///
    /// ```
    /// use ds_api::ApiRequest;
    ///
    /// let req = ApiRequest::builder().with_model("deepseek-chat");
    /// let req = ApiRequest::builder().with_model("gpt-4o");
    /// ```
    pub fn with_model(mut self, name: impl Into<String>) -> Self {
        self.raw.model = crate::raw::Model::Custom(name.into());
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

    /// Merge arbitrary top-level JSON into the request body.
    ///
    /// Pass a `serde_json::Map<String, serde_json::Value>` of key/value pairs which
    /// will be flattened into the top-level request JSON via the raw request's
    /// `extra_body` field.
    pub fn extra_body(mut self, map: serde_json::Map<String, serde_json::Value>) -> Self {
        self.raw.extra_body = Some(map);
        self
    }

    /// Add a single extra top-level field to the request body (in-place).
    ///
    /// This method mutates the internal `ChatCompletionRequest`'s `extra_body`
    /// map, creating it if necessary, and inserts the provided `key`/`value`
    /// pair. Values in `extra_body` are flattened into the top-level request
    /// JSON when serialised due to `#[serde(flatten)]`, so they appear as peers
    /// to fields such as `messages` and `model`.
    ///
    /// Use this when you hold a mutable `ApiRequest` and want to add
    /// provider-specific or experimental top-level fields without constructing a
    /// full `Map` first.
    ///
    /// Example:
    ///
    /// ```rust
    /// # use ds_api::ApiRequest;
    /// # use serde_json::json;
    /// let mut req = ApiRequest::builder();
    /// req.add_extra_field("x_flag", json!(true));
    /// ```
    pub fn add_extra_field(&mut self, key: impl Into<String>, value: serde_json::Value) {
        if let Some(ref mut m) = self.raw.extra_body {
            m.insert(key.into(), value);
        } else {
            let mut m = serde_json::Map::new();
            m.insert(key.into(), value);
            self.raw.extra_body = Some(m);
        }
    }

    /// Builder-style helper that consumes the `ApiRequest`, adds an extra field,
    /// and returns the modified request for chaining.
    ///
    /// This is convenient for fluent construction:
    ///
    /// ```rust
    /// # use ds_api::ApiRequest;
    /// # use serde_json::json;
    /// let req = ApiRequest::builder()
    ///     .with_extra_field("provider_opt", json!("x"))
    ///     .with_model("deepseek-chat");
    /// ```
    pub fn with_extra_field(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.add_extra_field(key, value);
        self
    }

    /// Compatibility alias for the single-field helper (builder-style).
    ///
    /// Historically the builder exposed `extra_field(...)`. This method is kept
    /// as an alias for compatibility but prefer `with_extra_field` or
    /// `add_extra_field` for clearer intent.
    pub fn extra_field(self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.with_extra_field(key, value)
    }

    /// Build and return the internal raw request (crate-internal use).
    pub(crate) fn into_raw(self) -> ChatCompletionRequest {
        self.raw
    }
}
