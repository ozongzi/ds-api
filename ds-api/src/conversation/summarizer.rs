//! Summarizer submodule for conversation.
//!
//! Contains the `Summarizer` trait and a default `TokenBasedSummarizer`.
//! This module is intended to be used by the conversation implementation.

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::request::message::{Message, Role};

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
}
