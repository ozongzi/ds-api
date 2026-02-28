use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::{StreamExt, stream::BoxStream};
use reqwest::Client;

use super::request::ApiRequest;
use crate::error::{ApiError, Result};
use crate::raw::{ChatCompletionChunk, ChatCompletionResponse};

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
