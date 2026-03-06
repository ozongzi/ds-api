use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::{StreamExt, stream::BoxStream};
use reqwest::Client;

use tracing::{debug, info, instrument, warn};

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
    #[instrument(level = "info", skip(token), fields(masked_token = tracing::field::Empty))]
    pub fn new(token: impl Into<String>) -> Self {
        // avoid recording the raw token value in traces; we mark a field instead
        let token_str = token.into();
        info!(message = "creating ApiClient instance");
        let client = Self {
            token: token_str.clone(),
            base_url: "https://api.deepseek.com".to_string(),
            client: Client::new(),
            timeout: None,
        };
        // annotate the trace/span with a masked token indicator (presence only)
        tracing::Span::current().record("masked_token", &"***");
        client
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
    #[instrument(level = "info", skip(self, req))]
    pub async fn send(&self, req: ApiRequest) -> Result<ChatCompletionResponse> {
        let raw = req.into_raw();
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        debug!(method = "POST", %url, "sending non-streaming request");

        let mut builder = self.client.post(&url).bearer_auth(&self.token).json(&raw);
        if let Some(t) = self.timeout {
            builder = builder.timeout(t);
            debug!(timeout_ms = ?t.as_millis(), "request timeout set");
        }

        let resp = match builder.send().await {
            Ok(r) => {
                debug!("received HTTP response");
                r
            }
            Err(e) => {
                warn!(error = %e, "http send failed");
                return Err(ApiError::Reqwest(e));
            }
        };

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_else(|e| e.to_string());
            warn!(%status, "non-success response");
            return Err(ApiError::http_error(status, text));
        }

        let parsed = resp.json::<ChatCompletionResponse>().await.map_err(|e| {
            warn!(error = %e, "failed to parse ChatCompletionResponse");
            ApiError::Reqwest(e)
        })?;
        info!("request completed successfully");
        Ok(parsed)
    }

    /// Send a streaming (SSE) request and return a boxed pinned stream of parsed `ChatCompletionChunk`.
    #[instrument(level = "info", skip(self, req))]
    pub async fn send_stream(
        &self,
        req: ApiRequest,
    ) -> Result<BoxStream<'_, std::result::Result<ChatCompletionChunk, ApiError>>> {
        let mut raw = req.into_raw();
        raw.stream = Some(true);

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        debug!(method = "POST", %url, "sending streaming request");
        let response = match self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&raw)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "stream http send failed");
                return Err(ApiError::Reqwest(e));
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_else(|e| e.to_string());
            warn!(%status, "non-success response for stream");
            return Err(ApiError::http_error(status, text));
        }

        // Convert to SSE event stream
        let event_stream = response.bytes_stream().eventsource();
        info!("stream connected; converting SSE to chunk stream");

        // Map SSE events -> parsed ChatCompletionChunk or ApiError
        let chunk_stream = event_stream.filter_map(|ev_res| async move {
            match ev_res {
                Ok(ev) => {
                    if ev.data == "[DONE]" {
                        debug!("received [DONE] event");
                        None
                    } else {
                        match serde_json::from_str::<ChatCompletionChunk>(&ev.data) {
                            Ok(chunk) => {
                                debug!("parsed chunk");
                                Some(Ok(chunk))
                            }
                            Err(e) => {
                                warn!(error = %e, "failed to parse chunk");
                                Some(Err(ApiError::Json(e)))
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "eventsource error");
                    Some(Err(ApiError::EventSource(e.to_string())))
                }
            }
        });

        // Box the stream into a pinned BoxStream for ergonomic returns.
        Ok(chunk_stream.boxed())
    }

    /// Send a streaming request consuming `self`, returning a `'static` `BoxStream`.
    ///
    /// Unlike `send_stream`, this takes ownership so the returned stream is not tied
    /// to a client lifetime — useful when the stream must outlive the client reference
    /// (e.g. storing it inside a state machine).
    #[instrument(level = "info", skip(self, req))]
    pub async fn into_stream(
        self,
        req: ApiRequest,
    ) -> Result<BoxStream<'static, std::result::Result<ChatCompletionChunk, ApiError>>> {
        let mut raw = req.into_raw();
        raw.stream = Some(true);

        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        debug!(method = "POST", %url, "sending streaming request (owned)");

        let response = match self
            .client
            .post(&url)
            .bearer_auth(&self.token)
            .json(&raw)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                warn!(error = %e, "stream http send failed");
                return Err(ApiError::Reqwest(e));
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_else(|e| e.to_string());
            warn!(%status, "non-success response for stream (owned)");
            return Err(ApiError::http_error(status, text));
        }

        let event_stream = response.bytes_stream().eventsource();
        info!("stream connected (owned); converting SSE to chunk stream");

        let chunk_stream = event_stream.filter_map(|ev_res| async move {
            match ev_res {
                Ok(ev) => {
                    if ev.data == "[DONE]" {
                        debug!("received [DONE] event");
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

        Ok(chunk_stream.boxed())
    }

    /// Convenience: stream only text fragments (delta.content) as String items.
    ///
    /// Each yielded item is `Result<String, ApiError>`.
    #[instrument(level = "debug", skip(self, req))]
    pub async fn stream_text(
        &self,
        req: ApiRequest,
    ) -> Result<BoxStream<'_, std::result::Result<String, ApiError>>> {
        debug!("creating text stream from chunk stream");
        let chunk_stream = self.send_stream(req).await?;
        let text_stream = chunk_stream.map(|item_res| match item_res {
            Ok(chunk) => {
                let s = chunk
                    .choices
                    .first()
                    .and_then(|c| c.delta.content.as_ref())
                    .cloned()
                    .unwrap_or_default();
                debug!(fragment = %s, "yielding text fragment");
                Ok(s)
            }
            Err(e) => {
                warn!(error = %e, "yielding error from chunk stream");
                Err(e)
            }
        });

        Ok(text_stream.boxed())
    }
}
