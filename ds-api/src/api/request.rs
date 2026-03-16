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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::{Function, FunctionCall, Role, ToolCall, ToolType};
    use serde_json::json;

    #[test]
    fn builder_variants_and_message_mutation_work() {
        let req = ApiRequest::builder()
            .with_model("gpt-4o-mini")
            .add_message(Message::user("u1"))
            .add_message(Message::assistant("a1"))
            .messages(vec![Message::system("s1")])
            .stream(true)
            .temperature(0.3)
            .max_tokens(128)
            .tool_choice_auto()
            .text()
            .into_raw();

        assert!(matches!(req.model, crate::raw::Model::Custom(ref m) if m == "gpt-4o-mini"));
        assert_eq!(req.messages.len(), 1);
        assert!(matches!(req.messages[0].role, Role::System));
        assert_eq!(req.stream, Some(true));
        assert_eq!(req.temperature, Some(0.3));
        assert_eq!(req.max_tokens, Some(128));
        assert!(matches!(
            req.tool_choice,
            Some(crate::raw::request::tool_choice::ToolChoice::String(
                crate::raw::request::tool_choice::ToolChoiceType::Auto
            ))
        ));
        assert!(matches!(
            req.response_format,
            Some(ResponseFormat {
                r#type: ResponseFormatType::Text
            })
        ));
    }

    #[test]
    fn deepseek_constructors_tools_and_extra_body_work() {
        let tool = Tool {
            r#type: ToolType::Function,
            function: Function {
                name: "echo".into(),
                description: Some("echo input".into()),
                parameters: json!({"type":"object","properties":{}}),
                strict: Some(true),
            },
        };

        let req = ApiRequest::deepseek_chat(vec![Message::user("hello")])
            .add_tool(tool.clone())
            .add_tool(tool)
            .json()
            .extra_body(serde_json::Map::from_iter([(String::from("x"), json!(1))]))
            .with_extra_field("y", json!(2))
            .extra_field("z", json!(3))
            .into_raw();

        assert!(matches!(req.model, crate::raw::Model::DeepseekChat));
        assert_eq!(req.tools.as_ref().map(Vec::len), Some(2));
        assert!(matches!(
            req.response_format,
            Some(ResponseFormat {
                r#type: ResponseFormatType::JsonObject
            })
        ));
        assert_eq!(
            req.extra_body.as_ref().and_then(|m| m.get("x")),
            Some(&json!(1))
        );
        assert_eq!(
            req.extra_body.as_ref().and_then(|m| m.get("y")),
            Some(&json!(2))
        );
        assert_eq!(
            req.extra_body.as_ref().and_then(|m| m.get("z")),
            Some(&json!(3))
        );

        let reasoner = ApiRequest::deepseek_reasoner(vec![Message::assistant("a")]).into_raw();
        assert!(matches!(
            reasoner.model,
            crate::raw::Model::DeepseekReasoner
        ));
    }

    #[test]
    fn add_extra_field_mutates_existing_or_new_map() {
        let mut req = ApiRequest::builder();
        req.add_extra_field("a", json!("v1"));
        req.add_extra_field("b", json!(false));
        let raw = req.into_raw();

        assert_eq!(
            raw.extra_body.as_ref().and_then(|m| m.get("a")),
            Some(&json!("v1"))
        );
        assert_eq!(
            raw.extra_body.as_ref().and_then(|m| m.get("b")),
            Some(&json!(false))
        );
    }

    #[test]
    fn tool_call_message_shape_serializes_in_request() {
        let call = ToolCall {
            id: "call_1".into(),
            r#type: ToolType::Function,
            function: FunctionCall {
                name: "echo".into(),
                arguments: r#"{"input":"hi"}"#.into(),
            },
        };

        let mut assistant = Message::assistant("");
        assistant.content = None;
        assistant.tool_calls = Some(vec![call]);

        let raw = ApiRequest::builder().add_message(assistant).into_raw();
        let v = serde_json::to_value(&raw).unwrap();
        assert_eq!(
            v["messages"][0]["tool_calls"][0]["function"]["name"],
            "echo"
        );
    }
}
