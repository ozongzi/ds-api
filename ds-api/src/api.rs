/*
ds-api-workspace/ds-api/src/api.rs

High-level API module.

Provides:
- `ApiRequest`: a safe, chainable builder that wraps the raw `ChatCompletionRequest`.
  It intentionally does not expose the raw `Model` enum to callers. Use
  `deepseek_chat(...)` and `deepseek_reasoner(...)` to choose the model.
- `ApiClient`: lightweight HTTP client wrapper for sending requests and receiving
  both non-streaming and streaming (SSE) responses.

Internals still use `crate::raw` types, but these are not required by most users.
*/

use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::{StreamExt, stream::BoxStream};
use reqwest::Client;

use crate::error::{ApiError, Result};

use crate::raw::{
    ChatCompletionChunk, ChatCompletionRequest, ChatCompletionResponse, Message, ResponseFormat,
    ResponseFormatType, Tool,
};

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

/// Lightweight API HTTP client.
#[derive(Clone, Debug)]
pub struct ApiClient {
    token: String,
    base_url: String,
    client: Client,
    timeout: Option<Duration>,
}

impl ApiClient {
    /// Create a new client with the given token.
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            base_url: "https://api.deepseek.com".to_string(),
            client: Client::new(),
            timeout: None,
        }
    }

    /// Replace base URL (builder style).
    pub fn with_base_url(mut self, base: impl Into<String>) -> Self {
        self.base_url = base.into();
        self
    }

    /// Replace token (builder style).
    pub fn with_token(mut self, token: impl Into<String>) -> Self {
        self.token = token.into();
        self
    }

    /// Set optional timeout for non-streaming requests.
    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.timeout = Some(t);
        self
    }

    /// Send a non-streaming request and parse the full ChatCompletionResponse.
    pub async fn send(&self, req: ApiRequest) -> Result<ChatCompletionResponse> {
        let raw = req.into_raw();
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let mut builder = self.client.post(&url).bearer_auth(&self.token).json(&raw);
        if let Some(t) = self.timeout {
            builder = builder.timeout(t);
        }

        let resp = builder.send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_else(|e| e.to_string());
            return Err(ApiError::http_error(status, text));
        }

        let parsed = resp.json::<ChatCompletionResponse>().await?;
        Ok(parsed)
    }

    /// Send a streaming (SSE) request and return a boxed pinned stream of parsed `ChatCompletionChunk`.
    pub async fn send_stream(
        &self,
        req: ApiRequest,
    ) -> Result<BoxStream<'_, std::result::Result<ChatCompletionChunk, ApiError>>> {
        let mut raw = req.into_raw();
        raw.stream = Some(true);

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let response = self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&raw)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_else(|e| e.to_string());
            return Err(ApiError::http_error(status, text));
        }

        // Convert to SSE event stream
        let event_stream = response.bytes_stream().eventsource();

        // Map SSE events -> parsed ChatCompletionChunk or ApiError
        let chunk_stream = event_stream.filter_map(|ev_res| async move {
            match ev_res {
                Ok(ev) => {
                    if ev.data == "[DONE]" {
                        None
                    } else {
                        match serde_json::from_str::<ChatCompletionChunk>(&ev.data) {
                            Ok(chunk) => Some(Ok(chunk)),
                            Err(e) => Some(Err(ApiError::Json(e))),
                        }
                    }
                }
                Err(e) => Some(Err(ApiError::EventSource(e.to_string()))),
            }
        });

        // Box the stream into a pinned BoxStream for ergonomic returns.
        Ok(chunk_stream.boxed())
    }

    /// Convenience: stream only text fragments (delta.content) as String items.
    ///
    /// Each yielded item is `Result<String, ApiError>`.
    pub async fn stream_text(
        &self,
        req: ApiRequest,
    ) -> Result<BoxStream<'_, std::result::Result<String, ApiError>>> {
        let chunk_stream = self.send_stream(req).await?;

        let text_stream = chunk_stream.map(|item_res| match item_res {
            Ok(chunk) => {
                let s = chunk
                    .choices
                    .first()
                    .and_then(|c| c.delta.content.as_ref())
                    .cloned()
                    .unwrap_or_default();
                Ok(s)
            }
            Err(e) => Err(e),
        });

        Ok(text_stream.boxed())
    }
}
