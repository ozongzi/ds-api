/*
ds-api-workspace/ds-api/src/conversation/mod.rs

Conversation module:
- Defines a `Summarizer` trait and a default `TokenBasedSummarizer` which triggers when
  estimated tokens exceed 100_000 and replaces old messages with a single system summary.
- Implements `DeepseekConversation` which holds history, an ApiClient and a Summarizer.
- Conversation streaming helper was moved from the `Conversation` trait into an
  inherent async method on `DeepseekConversation` to avoid lifetime complexity and
  to keep the trait object-safe and small.
*/

use std::future::Future;
use std::pin::Pin;

use futures::stream::BoxStream;

use crate::api::{ApiClient, ApiRequest};
use crate::error::{ApiError, Result};
use crate::raw::request::message::{Message, Role};

/// Summarizer trait:
/// - `should_summarize` checks whether current history should be summarized
/// - `summarize` mutates the history to replace older messages with a single short `system` summary message
pub trait Summarizer: Send + Sync {
    /// Return true if history should be summarized now.
    fn should_summarize(&self, history: &[Message]) -> bool;

    /// Perform summarization by mutating `history`.
    /// If a summary was created and applied, return `Some(Message)` representing the inserted summary message.
    /// If nothing was done, return `None`.
    fn summarize(&self, history: &mut Vec<Message>) -> Option<Message>;
}

/// Default token-based summarizer:
/// - Estimates tokens roughly as total characters / 4
/// - Triggers when estimated tokens exceed `threshold` (default 100_000)
/// - Keeps `retain_last` most recent messages and summarizes the older ones into a single system message.
/// - The summary is a concatenation/truncation of older messages, not an LLM semantic summary.
#[derive(Clone, Debug)]
pub struct TokenBasedSummarizer {
    pub threshold: usize,
    pub retain_last: usize,
    pub max_summary_chars: usize,
}

impl Default for TokenBasedSummarizer {
    fn default() -> Self {
        Self {
            threshold: 100_000,
            retain_last: 10,
            max_summary_chars: 2_000, // cap the summary length
        }
    }
}

impl TokenBasedSummarizer {
    /// A simple heuristic to estimate tokens from message history.
    /// Uses chars / 4 as a rough token estimate.
    fn estimate_tokens(history: &[Message]) -> usize {
        // Skip counting system messages (system prompts) when estimating tokens.
        history
            .iter()
            .filter(|m| !matches!(m.role, Role::System))
            .filter_map(|m| m.content.as_ref())
            .map(|s| s.len())
            .sum::<usize>()
            / 4
    }
}

impl Summarizer for TokenBasedSummarizer {
    fn should_summarize(&self, history: &[Message]) -> bool {
        let est = Self::estimate_tokens(history);
        est >= self.threshold
    }

    fn summarize(&self, history: &mut Vec<Message>) -> Option<Message> {
        if history.len() <= self.retain_last {
            return None;
        }

        // Determine slice to summarize (old messages)
        let split = history.len().saturating_sub(self.retain_last);
        if split == 0 {
            return None;
        }

        // Collect older messages for summarization
        let older: Vec<String> = history.drain(0..split).filter_map(|m| m.content).collect();

        if older.is_empty() {
            // nothing meaningful to summarize; nothing to do
            return None;
        }

        // Simple concatenation with newlines; truncate if too long.
        let joined = older.join("\n");
        let summary_text = if joined.len() > self.max_summary_chars {
            let mut s = joined;
            s.truncate(self.max_summary_chars);
            format!("Short summary of earlier conversation:\n{}\n(Truncated)", s)
        } else {
            format!("Short summary of earlier conversation:\n{}", joined)
        };

        // Create a system message as the summary and insert at the front.
        let mut summary_msg = Message::new(Role::System, summary_text.as_str());
        // Optionally tag name to indicate it's an auto-summary
        summary_msg.name = Some("[auto-summary]".to_string());

        history.insert(0, summary_msg.clone());
        Some(summary_msg)
    }
}

/// Conversation trait: async-friendly via boxed futures so implementors can perform network calls.
pub trait Conversation {
    /// Get a reference to the conversation history.
    fn history(&self) -> &Vec<Message>;

    /// Add an arbitrary message to the history (any role).
    fn add_message(&mut self, message: Message);

    /// Push a user input into history (convenience for typical flows).
    fn push_user_input(&mut self, text: String);

    /// Optionally trigger summarization immediately.
    fn maybe_summarize(&mut self);

    /// Send the current history as a single request and return the assistant's content (if any).
    /// This returns a boxed future so the trait remains object-safe without additional crates.
    fn send_once<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send + 'a>>;
}

/// DeepseekConversation: a concrete Conversation implementation.
/// - owns an `ApiClient` (used to send requests)
/// - stores `history: Vec<Message>`
/// - holds a `Summarizer` implementation (boxed)
/// - supports auto-summary toggle
pub struct DeepseekConversation {
    client: ApiClient,
    history: Vec<Message>,
    summarizer: Box<dyn Summarizer + Send + Sync>,
    auto_summary: bool,
}

impl DeepseekConversation {
    /// Create a conversation with an ApiClient and default summarizer.
    pub fn new(client: ApiClient) -> Self {
        Self {
            client,
            history: vec![],
            summarizer: Box::new(TokenBasedSummarizer::default()),
            auto_summary: true,
        }
    }

    /// Builder: set a custom summarizer
    // `Summarizer` already requires `Send + Sync`; avoid repeating implied bounds here.
    pub fn with_summarizer(mut self, s: impl Summarizer + 'static) -> Self {
        self.summarizer = Box::new(s);
        self
    }

    /// Builder: enable or disable auto-summary behavior
    pub fn enable_auto_summary(mut self, v: bool) -> Self {
        self.auto_summary = v;
        self
    }

    /// Builder: seed conversation history with initial messages
    pub fn with_history(mut self, history: Vec<Message>) -> Self {
        self.history = history;
        self
    }

    /// Inspect mutable history (advanced use)
    pub fn history_mut(&mut self) -> &mut Vec<Message> {
        &mut self.history
    }

    /// Internal helper that checks and runs summarization if needed.
    fn maybe_do_summary(&mut self) {
        if self.auto_summary && self.summarizer.should_summarize(&self.history) {
            self.summarizer.summarize(&mut self.history);
        }
    }
}

impl Conversation for DeepseekConversation {
    fn history(&self) -> &Vec<Message> {
        &self.history
    }

    fn add_message(&mut self, message: Message) {
        self.history.push(message);
        // After adding an arbitrary message, optionally summarize
        self.maybe_do_summary();
    }

    fn push_user_input(&mut self, text: String) {
        // push owned string directly into history as a User message
        self.history.push(Message::new(Role::User, text.as_str()));
        // Optionally perform summary check eagerly after user input
        self.maybe_do_summary();
    }

    fn maybe_summarize(&mut self) {
        self.maybe_do_summary();
    }

    fn send_once<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send + 'a>> {
        Box::pin(async move {
            // build ApiRequest from current history (use deepseek_chat by default)
            // We choose the deepseek_chat constructor to be the default model.
            let req = ApiRequest::builder().messages(self.history.clone());
            let resp = self.client.send(req).await?;
            // extract first choice
            let choice = resp
                .choices
                .into_iter()
                .next()
                .ok_or_else(|| ApiError::Other("empty choices from API".to_string()))?;
            let assistant_msg = choice.message;
            let content = assistant_msg.content.clone();

            // append assistant message to history
            self.history.push(assistant_msg);

            // maybe summarize after adding assistant
            self.maybe_do_summary();

            Ok(content)
        })
    }

    // streaming helper was intentionally removed from the trait and is provided
    // as an inherent async method on `DeepseekConversation`.
}

impl DeepseekConversation {
    /// Stream text fragments (delta.content) as a boxed stream of `Result<String, ApiError>`.
    ///
    /// This is an inherent async method (not part of the `Conversation` trait) to avoid
    /// trait object lifetime complexity. It simply delegates to the underlying ApiClient.
    pub async fn stream_text(
        &mut self,
    ) -> Result<BoxStream<'_, std::result::Result<String, ApiError>>> {
        let req = ApiRequest::builder()
            .messages(self.history.clone())
            .stream(true);
        let stream = self.client.stream_text(req).await?;
        Ok(stream)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiClient;
    use crate::raw::request::message::Role;

    // Note: These tests are lightweight and do not perform network calls.
    #[test]
    fn summarizer_should_trigger_and_replace() {
        let mut hist = vec![
            Message::new(Role::User, "User message 1"),
            Message::new(Role::Assistant, "Assistant reply 1"),
            Message::new(Role::User, "User message 2"),
            Message::new(Role::Assistant, "Assistant reply 2"),
        ];

        // Use a very low threshold so summarization triggers.
        let summ = TokenBasedSummarizer {
            threshold: 0, // everything triggers
            retain_last: 1,
            max_summary_chars: 100,
        };

        assert!(summ.should_summarize(&hist));
        let maybe_summary = summ.summarize(&mut hist);
        assert!(maybe_summary.is_some());
        // After summarization, history length should be <= retain_last + 1 (summary)
        assert!(hist.len() <= (1 + 1));
        // First message must be a system message (the summary)
        assert!(matches!(hist[0].role, Role::System));
    }

    #[test]
    fn conversation_builder_and_push() {
        let client = ApiClient::new("fake-token");
        let conv = DeepseekConversation::new(client).with_history(vec![]);
        assert!(conv.history().is_empty());
    }
}
