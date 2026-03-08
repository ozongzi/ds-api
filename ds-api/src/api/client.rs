use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::{StreamExt, stream::BoxStream};
use reqwest::{Client, Response};

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
        Self::with_client(token, Client::new())
    }

    #[instrument(level = "info", skip(token), fields(masked_token = tracing::field::Empty))]
    pub fn with_client(token: impl Into<String>, client: Client) -> Self {
        let token_str = token.into();
        info!(message = "creating ApiClient instance");
        let client = Self {
            token: token_str,
            base_url: "https://api.deepseek.com".to_string(),
            client,
            timeout: None,
        };
        tracing::Span::current().record("masked_token", "***");
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

    // ── Private helpers ───────────────────────────────────────────────────────

    fn completions_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }

    /// Send an HTTP POST to the completions endpoint and return the raw
    /// [`Response`], checking for non-2xx status codes.
    async fn post_raw(&self, req: ApiRequest, stream: bool) -> Result<Response> {
        let mut raw = req.into_raw();
        if stream {
            raw.stream = Some(true);
        }

        let url = self.completions_url();
        debug!(method = "POST", %url, %stream, "sending request");

        let mut builder = self.client.post(&url).bearer_auth(&self.token).json(&raw);
        if !stream && let Some(t) = self.timeout {
            builder = builder.timeout(t);
            debug!(timeout_ms = ?t.as_millis(), "request timeout set");
        }

        let resp = builder.send().await.map_err(|e| {
            warn!(error = %e, "http send failed");
            ApiError::Reqwest(e)
        })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_else(|e| e.to_string());
            warn!(%status, "non-success response");
            return Err(ApiError::http_error(status, text));
        }

        Ok(resp)
    }

    /// Convert a successful streaming [`Response`] into a
    /// `BoxStream<Result<ChatCompletionChunk, ApiError>>`.
    ///
    /// This is the single source of truth for SSE → chunk parsing.
    fn response_into_chunk_stream(
        resp: Response,
    ) -> BoxStream<'static, std::result::Result<ChatCompletionChunk, ApiError>> {
        let event_stream = resp.bytes_stream().eventsource();

        event_stream
            .filter_map(|ev_res| async move {
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
            })
            .boxed()
    }

    // ── Public API ────────────────────────────────────────────────────────────

    /// Send a non-streaming request and parse the full [`ChatCompletionResponse`].
    #[instrument(level = "info", skip(self, req))]
    pub async fn send(&self, req: ApiRequest) -> Result<ChatCompletionResponse> {
        let resp = self.post_raw(req, false).await?;
        debug!("received HTTP response; deserialising");

        let parsed = resp.json::<ChatCompletionResponse>().await.map_err(|e| {
            warn!(error = %e, "failed to parse ChatCompletionResponse");
            ApiError::Reqwest(e)
        })?;

        info!("request completed successfully");
        Ok(parsed)
    }

    /// Send a streaming (SSE) request and return a `BoxStream` of parsed
    /// [`ChatCompletionChunk`]s.
    ///
    /// The stream borrows `&self` via the response lifetime, so it cannot
    /// outlive the client.  Use [`into_stream`][Self::into_stream] when you
    /// need a `'static` stream (e.g. inside a state machine that owns the
    /// client).
    #[instrument(level = "info", skip(self, req))]
    pub async fn send_stream(
        &self,
        req: ApiRequest,
    ) -> Result<BoxStream<'_, std::result::Result<ChatCompletionChunk, ApiError>>> {
        let resp = self.post_raw(req, true).await?;
        info!("stream connected");
        Ok(Self::response_into_chunk_stream(resp))
    }

    /// Send a streaming (SSE) request, consuming `self`, and return a
    /// `'static` `BoxStream` of parsed [`ChatCompletionChunk`]s.
    ///
    /// Taking ownership of the client means the returned stream is not tied to
    /// any borrow, making it suitable for storage inside a state machine (e.g.
    /// [`AgentStream`][crate::agent::AgentStream]).
    #[instrument(level = "info", skip(self, req))]
    pub async fn into_stream(
        self,
        req: ApiRequest,
    ) -> Result<BoxStream<'static, std::result::Result<ChatCompletionChunk, ApiError>>> {
        let resp = self.post_raw(req, true).await?;
        info!("stream connected (owned)");
        Ok(Self::response_into_chunk_stream(resp))
    }

    /// Convenience: stream only text fragments (`delta.content`) as [`String`]
    /// items.
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
