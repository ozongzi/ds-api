//! The `Conversation` struct — manages history and context-window compression.

use futures::stream::BoxStream;

use crate::api::{ApiClient, ApiRequest};
use crate::error::{ApiError, Result};
use crate::raw::request::message::{Message, Role};

use crate::conversation::{LlmSummarizer, Summarizer};

/// Maintains a conversation history and handles context-window compression.
///
/// This is the primary building block used by [`DeepseekAgent`][crate::agent::DeepseekAgent].
/// You can also use it directly for simple back-and-forth conversations that do not need tools.
///
/// # Context management
///
/// By default the conversation uses [`LlmSummarizer`], which calls DeepSeek to write
/// a concise summary of older turns once the estimated token count exceeds a threshold.
/// Swap it out via [`with_summarizer`][Conversation::with_summarizer]:
///
/// ```no_run
/// use ds_api::{ApiClient, conversation::Conversation, conversation::SlidingWindowSummarizer};
///
/// let conv = Conversation::new(ApiClient::new("sk-..."))
///     .with_summarizer(SlidingWindowSummarizer::new(20));
/// ```
pub struct Conversation {
    pub(crate) client: ApiClient,
    pub(crate) history: Vec<Message>,
    summarizer: Box<dyn Summarizer + Send + Sync>,
    auto_summary: bool,
}

impl Conversation {
    /// Create a new conversation backed by `client`.
    ///
    /// The default summarizer is [`LlmSummarizer`] with sensible defaults
    /// (~60 000 estimated tokens trigger, retain last 10 turns).
    pub fn new(client: ApiClient) -> Self {
        let summarizer = LlmSummarizer::new(client.clone());
        Self {
            client,
            history: vec![],
            summarizer: Box::new(summarizer),
            auto_summary: true,
        }
    }

    // ── Builder methods ───────────────────────────────────────────────────────

    /// Replace the summarizer.
    pub fn with_summarizer(mut self, s: impl Summarizer + 'static) -> Self {
        self.summarizer = Box::new(s);
        self
    }

    /// Enable or disable automatic summarization (enabled by default).
    pub fn enable_auto_summary(mut self, v: bool) -> Self {
        self.auto_summary = v;
        self
    }

    /// Seed the conversation with an existing message history.
    pub fn with_history(mut self, history: Vec<Message>) -> Self {
        self.history = history;
        self
    }

    // ── History access ────────────────────────────────────────────────────────

    /// Read-only view of the current history.
    pub fn history(&self) -> &[Message] {
        &self.history
    }

    /// Mutable access to the raw history (advanced use).
    pub fn history_mut(&mut self) -> &mut Vec<Message> {
        &mut self.history
    }

    // ── Mutation helpers ──────────────────────────────────────────────────────

    /// Append an arbitrary message (any role) to the history.
    pub fn add_message(&mut self, message: Message) {
        self.history.push(message);
    }

    /// Append a `Role::User` message to the history.
    pub fn push_user_input(&mut self, text: impl Into<String>) {
        self.history.push(Message::new(Role::User, &text.into()));
    }

    // ── Summarization ─────────────────────────────────────────────────────────

    /// Run the summarizer if the current history warrants it.
    ///
    /// Errors from the summarizer are silently swallowed so that a transient API
    /// failure during summarization does not abort an ongoing conversation turn.
    pub async fn maybe_summarize(&mut self) {
        if !self.auto_summary {
            return;
        }
        if !self.summarizer.should_summarize(&self.history) {
            return;
        }
        let _ = self.summarizer.summarize(&mut self.history).await;
    }

    // ── Single-turn send ──────────────────────────────────────────────────────

    /// Send the current history to the API as a single (non-streaming) request
    /// and return the assistant's text content (if any).
    ///
    /// The assistant reply is automatically appended to the history.
    /// Summarization is run both before the request and after the reply is received.
    pub async fn send_once(&mut self) -> Result<Option<String>> {
        self.maybe_summarize().await;

        let req = ApiRequest::builder().messages(self.history.clone());
        let resp = self.client.send(req).await?;

        let choice = resp
            .choices
            .into_iter()
            .next()
            .ok_or_else(|| ApiError::Other("empty choices from API".to_string()))?;

        let assistant_msg = choice.message;
        let content = assistant_msg.content.clone();
        self.history.push(assistant_msg);

        self.maybe_summarize().await;

        Ok(content)
    }

    /// Stream text fragments (`delta.content`) from the API as a
    /// `BoxStream<Result<String, ApiError>>`.
    ///
    /// Unlike [`send_once`][Conversation::send_once], this method does **not**
    /// automatically append the assistant reply or run summarization — the caller
    /// is responsible for collecting the stream and updating history if needed.
    pub async fn stream_text(
        &mut self,
    ) -> Result<BoxStream<'_, std::result::Result<String, ApiError>>> {
        let req = ApiRequest::builder()
            .messages(self.history.clone())
            .stream(true);
        self.client.stream_text(req).await
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fake() -> Conversation {
        Conversation::new(ApiClient::new("fake-token"))
    }

    #[test]
    fn new_has_empty_history() {
        assert!(fake().history().is_empty());
    }

    #[test]
    fn with_history_seeds_messages() {
        let msgs = vec![Message::new(Role::User, "hi")];
        let conv = fake().with_history(msgs);
        assert_eq!(conv.history().len(), 1);
    }

    #[test]
    fn push_user_input_appends_user_role() {
        let mut conv = fake();
        conv.push_user_input("hello");
        assert_eq!(conv.history().len(), 1);
        assert!(matches!(conv.history()[0].role, Role::User));
    }

    #[test]
    fn add_message_appends() {
        let mut conv = fake();
        conv.add_message(Message::new(Role::Assistant, "hi"));
        assert_eq!(conv.history().len(), 1);
        assert!(matches!(conv.history()[0].role, Role::Assistant));
    }

    #[test]
    fn enable_auto_summary_false() {
        let conv = fake().enable_auto_summary(false);
        assert!(!conv.auto_summary);
    }
}
