use std::future::Future;
use std::pin::Pin;

use futures::stream::BoxStream;

use crate::api::{ApiClient, ApiRequest};
use crate::error::{ApiError, Result};
use crate::raw::request::message::{Message, Role};

use crate::conversation::{Conversation, Summarizer, TokenBasedSummarizer};

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
            let _ = self.summarizer.summarize(&mut self.history);
        }
    }

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::ApiClient;

    #[test]
    fn conversation_builder_and_push() {
        let client = ApiClient::new("fake-token");
        let conv = DeepseekConversation::new(client).with_history(vec![]);
        assert!(conv.history().is_empty());
    }
}
