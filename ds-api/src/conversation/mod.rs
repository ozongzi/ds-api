/*
ds-api-workspace/ds-api/src/conversation/mod.rs

Conversation module (refactored):

This module now delegates implementations to submodules:
- `summarizer` contains `Summarizer` and `TokenBasedSummarizer`.
- `deepseek` contains `DeepseekConversation` and related implementations.

Public re-exports are preserved so the crate-level API remains stable.
*/

pub mod deepseek;
pub mod summarizer;

pub use deepseek::DeepseekConversation;
pub use summarizer::{Summarizer, TokenBasedSummarizer};

use std::future::Future;
use std::pin::Pin;

use crate::raw::request::message::Message;

/// Conversation trait: async-friendly via boxed futures so implementors can perform network calls.
///
/// Implementations live in the `deepseek` submodule.
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
    ) -> Pin<Box<dyn Future<Output = crate::error::Result<Option<String>>> + Send + 'a>>;
}

// Note: DeepseekConversation implementation, helper methods and tests are now located in
// `ds-api-workspace/ds-api/src/conversation/deepseek.rs`.
