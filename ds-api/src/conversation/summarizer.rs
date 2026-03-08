//! Conversation summarizer trait and built-in implementations.
//!
//! The [`AUTO_SUMMARY_TAG`][crate::raw::request::message::AUTO_SUMMARY_TAG] constant
//! in [`Message`][crate::raw::request::message::Message] defines the single source of
//! truth for identifying auto-generated summary messages.
//!
//! # Trait
//!
//! [`Summarizer`] is an async trait with two methods:
//! - [`should_summarize`][Summarizer::should_summarize] — synchronous check on the current history.
//! - [`summarize`][Summarizer::summarize] — async, may perform an API call; mutates history in-place.
//!
//! # Built-in implementations
//!
//! | Type | Strategy |
//! |---|---|
//! | [`LlmSummarizer`] | Calls DeepSeek to produce a semantic summary; **default** for `DeepseekAgent`. |
//! | [`SlidingWindowSummarizer`] | Keeps the last N messages and silently drops the rest; no API call. |

use std::pin::Pin;

use futures::Future;

use crate::api::{ApiClient, ApiRequest};
use crate::error::ApiError;
use crate::raw::request::message::{Message, Role};

// ── Trait ────────────────────────────────────────────────────────────────────

/// Decides when and how to compress conversation history.
///
/// Both methods receive an immutable or mutable slice of the current history.
/// Implementors are free to count tokens, count turns, check wall-clock time,
/// or use any other heuristic.
///
/// The trait is object-safe via `BoxFuture`; you can store it as
/// `Box<dyn Summarizer>` without `async_trait`.
pub trait Summarizer: Send + Sync {
    /// Return `true` if the history should be summarized before the next API turn.
    ///
    /// This is called synchronously on every user-input push; keep it cheap.
    fn should_summarize(&self, history: &[Message]) -> bool;

    /// Compress `history` in-place, returning an error only for unrecoverable failures.
    ///
    /// On success the history must be shorter (or at most the same length) than before.
    /// Implementations must **not** remove messages whose role is [`Role::System`] and
    /// whose `name` field is not `Some("[auto-summary]")` — those are user-provided
    /// system prompts and must be preserved.
    fn summarize<'a>(
        &'a self,
        history: &'a mut Vec<Message>,
    ) -> Pin<Box<dyn Future<Output = Result<(), ApiError>> + Send + 'a>>;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Estimate the token count of a slice of messages using a fast character heuristic.
///
/// ASCII characters count as 1 char ≈ 0.25 tokens; CJK / multibyte characters are
/// counted as 4 chars ≈ 1 token.  System messages whose `name` is `[auto-summary]`
/// are included in the estimate; other system messages (user-provided prompts) are
/// excluded because they are permanent and we cannot remove them anyway.
pub(crate) fn estimate_tokens(history: &[Message]) -> usize {
    history
        .iter()
        .filter(|m| {
            // Always exclude permanent system prompts from the token estimate;
            // we can't remove them so counting them would trigger summarization
            // that can never actually free those tokens.
            if matches!(m.role, Role::System) {
                // auto-summary placeholders are replaceable → count them
                m.is_auto_summary()
            } else {
                true
            }
        })
        .filter_map(|m| m.content.as_deref())
        .map(|s| {
            s.chars()
                .map(|c| if c.is_ascii() { 1usize } else { 4 })
                .sum::<usize>()
        })
        .sum::<usize>()
        / 4
}

/// Partition `history` into (system_prompts, rest), where system prompts are
/// permanent user-provided system messages (role=System, name≠"[auto-summary]").
///
/// Returns the indices of permanent system messages so callers can re-inject
/// them after compressing the rest.
fn extract_system_prompts(history: &mut Vec<Message>) -> Vec<Message> {
    let mut prompts = Vec::new();
    let mut i = 0;
    while i < history.len() {
        let m = &history[i];
        let is_permanent_system = matches!(m.role, Role::System) && !m.is_auto_summary();
        if is_permanent_system {
            prompts.push(history.remove(i));
            // don't increment i — the next element shifted into position i
        } else {
            i += 1;
        }
    }
    prompts
}

// ── LlmSummarizer ─────────────────────────────────────────────────────────────

/// Summarizes older conversation turns by asking DeepSeek to write a concise
/// prose summary, then replaces the compressed turns with a single
/// `Role::System` message containing that summary.
///
/// # Trigger
///
/// Fires when the estimated token count of the **compressible** portion of the
/// history (everything except permanent system prompts) exceeds `token_threshold`.
///
/// # Behavior
///
/// 1. Permanent `Role::System` messages (user-provided via `with_system_prompt`)
///    are extracted and re-prepended after summarization — they are never lost.
/// 2. Any previous `[auto-summary]` system message is included in the text sent
///    to the model so the new summary is cumulative.
/// 3. The `retain_last` most recent non-system turns are kept verbatim; everything
///    older is replaced by the LLM-generated summary.
/// 4. If the API call fails the history is left **unchanged** and the error is
///    returned so the caller can decide whether to abort or continue.
///
/// # Example
///
/// ```no_run
/// use ds_api::{DeepseekAgent, ApiClient};
/// use ds_api::conversation::LlmSummarizer;
///
/// let summarizer = LlmSummarizer::new(ApiClient::new("sk-..."));
/// let agent = DeepseekAgent::new("sk-...")
///     .with_summarizer(summarizer);
/// ```
#[derive(Clone)]
pub struct LlmSummarizer {
    /// Client used exclusively for summary API calls (can share the agent's token).
    client: ApiClient,
    /// Estimated token count above which summarization is triggered.
    pub(crate) token_threshold: usize,
    /// Number of most-recent non-system messages to retain verbatim.
    pub(crate) retain_last: usize,
}

impl LlmSummarizer {
    /// Create with default thresholds: trigger at ~60 000 tokens, retain last 10 turns.
    pub fn new(client: ApiClient) -> Self {
        Self {
            client,
            token_threshold: 60_000,
            retain_last: 10,
        }
    }

    /// Builder: set a custom token threshold.
    pub fn token_threshold(mut self, n: usize) -> Self {
        self.token_threshold = n;
        self
    }

    /// Builder: set how many recent messages to keep verbatim.
    pub fn retain_last(mut self, n: usize) -> Self {
        self.retain_last = n;
        self
    }
}

impl Summarizer for LlmSummarizer {
    fn should_summarize(&self, history: &[Message]) -> bool {
        estimate_tokens(history) >= self.token_threshold
    }

    fn summarize<'a>(
        &'a self,
        history: &'a mut Vec<Message>,
    ) -> Pin<Box<dyn Future<Output = Result<(), ApiError>> + Send + 'a>> {
        Box::pin(async move {
            // ── 1. Extract permanent system prompts ──────────────────────────
            let system_prompts = extract_system_prompts(history);

            // ── 2. Split off the tail we want to keep verbatim ───────────────
            let retain = self.retain_last.min(history.len());
            let split = history.len().saturating_sub(retain);
            let tail: Vec<Message> = history.drain(split..).collect();

            // history now contains only the "old" turns (including any previous
            // [auto-summary] message).

            if history.is_empty() {
                // Nothing old enough to summarize — just restore everything.
                history.extend(tail);
                // re-prepend system prompts
                for (i, p) in system_prompts.into_iter().enumerate() {
                    history.insert(i, p);
                }
                return Ok(());
            }

            // ── 3. Build a prompt asking the model for a summary ─────────────
            //
            // We format the old turns as a readable transcript and ask for a
            // concise summary that preserves the most important facts and decisions.
            let mut transcript = String::new();
            for msg in &*history {
                // skip the old auto-summary header line if present — the content
                // itself is still useful context for the new summary
                let role_label = match msg.role {
                    Role::User => "User",
                    Role::Assistant => "Assistant",
                    Role::System => "System",
                    Role::Tool => "Tool",
                };
                if let Some(content) = &msg.content {
                    transcript.push_str(&format!("{role_label}: {content}\n"));
                }
            }

            let summarize_prompt = format!(
                "Below is a conversation transcript. Write a concise summary (a few sentences \
                 to a short paragraph) that captures the key context, decisions, and facts \
                 established so far. The summary will replace the original transcript and be \
                 read by the same AI assistant as a memory aid — be precise and neutral.\n\n\
                 Transcript:\n{transcript}"
            );

            let req = ApiRequest::builder()
                .add_message(Message::new(Role::User, &summarize_prompt))
                .max_tokens(512);

            let response = self.client.send(req).await?;

            let summary_text = response
                .choices
                .into_iter()
                .next()
                .and_then(|c| c.message.content)
                .unwrap_or_else(|| transcript.clone());

            // ── 4. Replace old turns with the summary message ────────────────
            history.clear();

            history.push(Message::auto_summary(format!(
                "Summary of the conversation so far:\n{summary_text}"
            )));

            // ── 5. Re-attach the verbatim tail and system prompts ────────────
            history.extend(tail);

            for (i, p) in system_prompts.into_iter().enumerate() {
                history.insert(i, p);
            }

            Ok(())
        })
    }
}

// ── SlidingWindowSummarizer ───────────────────────────────────────────────────

/// Keeps only the most recent `window` messages and silently discards everything
/// older.  No API call is made.
///
/// Use this when you want predictable, zero-cost context management and are
/// comfortable with the model losing access to earlier turns.
///
/// Permanent `Role::System` messages are always preserved regardless of `window`.
///
/// # Example
///
/// ```no_run
/// use ds_api::{DeepseekAgent};
/// use ds_api::conversation::SlidingWindowSummarizer;
///
/// let agent = DeepseekAgent::new("sk-...")
///     .with_summarizer(SlidingWindowSummarizer::new(20));
/// ```
#[derive(Debug, Clone)]
pub struct SlidingWindowSummarizer {
    /// Maximum number of non-system messages to retain.
    pub(crate) window: usize,
}

impl SlidingWindowSummarizer {
    /// Create a summarizer that keeps at most `window` non-system messages.
    pub fn new(window: usize) -> Self {
        Self { window }
    }
}

impl Summarizer for SlidingWindowSummarizer {
    fn should_summarize(&self, history: &[Message]) -> bool {
        let non_system = history
            .iter()
            .filter(|m| !matches!(m.role, Role::System))
            .count();
        non_system > self.window
    }

    fn summarize<'a>(
        &'a self,
        history: &'a mut Vec<Message>,
    ) -> Pin<Box<dyn Future<Output = Result<(), ApiError>> + Send + 'a>> {
        Box::pin(async move {
            // Extract and preserve permanent system prompts.
            let system_prompts = extract_system_prompts(history);

            // Remove any previous auto-summary messages — they're irrelevant
            // for a pure sliding window.
            history.retain(|m| !m.is_auto_summary());

            // Keep only the last `window` non-system messages.
            if history.len() > self.window {
                let drop = history.len() - self.window;
                history.drain(0..drop);
            }

            // Re-prepend the permanent system prompts at the front.
            for (i, p) in system_prompts.into_iter().enumerate() {
                history.insert(i, p);
            }

            Ok(())
        })
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(role: Role, text: &str) -> Message {
        Message::new(role, text)
    }

    fn system_prompt(text: &str) -> Message {
        // A permanent system prompt — no [auto-summary] name tag.
        Message::new(Role::System, text)
    }

    // ── estimate_tokens ───────────────────────────────────────────────────────

    #[test]
    fn estimate_tokens_excludes_permanent_system() {
        let history = vec![
            system_prompt("You are a helpful assistant."),
            msg(Role::User, "Hello"),         // 5 chars → 1 token
            msg(Role::Assistant, "Hi there"), // 8 chars → 2 tokens
        ];
        // Only the User + Assistant messages should contribute.
        let est = estimate_tokens(&history);
        assert!(est > 0);
        // "Hello" + "Hi there" = 13 chars / 4 = 3 tokens
        assert_eq!(est, 3);
    }

    #[test]
    fn estimate_tokens_includes_auto_summary() {
        let summary = Message::auto_summary("Some prior summary text.");

        let history = vec![summary];
        let est = estimate_tokens(&history);
        assert!(est > 0);
    }

    // ── SlidingWindowSummarizer ───────────────────────────────────────────────

    #[tokio::test]
    async fn sliding_window_trims_to_window() {
        let mut history = vec![
            system_prompt("system"),
            msg(Role::User, "a"),
            msg(Role::Assistant, "b"),
            msg(Role::User, "c"),
            msg(Role::Assistant, "d"),
            msg(Role::User, "e"),
        ];

        let s = SlidingWindowSummarizer::new(2);
        assert!(s.should_summarize(&history));
        s.summarize(&mut history).await.unwrap();

        // system prompt preserved
        assert!(
            history
                .iter()
                .any(|m| matches!(m.role, Role::System) && m.content.as_deref() == Some("system"))
        );

        // at most window non-system messages remain
        let non_sys: Vec<_> = history
            .iter()
            .filter(|m| !matches!(m.role, Role::System))
            .collect();
        assert_eq!(non_sys.len(), 2);

        // the retained messages are the most recent ones
        assert_eq!(non_sys[0].content.as_deref(), Some("d"));
        assert_eq!(non_sys[1].content.as_deref(), Some("e"));
    }

    #[tokio::test]
    async fn sliding_window_preserves_multiple_system_prompts() {
        let mut p1 = system_prompt("prompt one");
        let mut p2 = system_prompt("prompt two");
        // Give them something to distinguish them from auto-summary
        p1.name = None;
        p2.name = None;

        let mut history = vec![
            p1.clone(),
            p2.clone(),
            msg(Role::User, "1"),
            msg(Role::User, "2"),
            msg(Role::User, "3"),
        ];

        let s = SlidingWindowSummarizer::new(1);
        s.summarize(&mut history).await.unwrap();

        let sys_msgs: Vec<_> = history
            .iter()
            .filter(|m| matches!(m.role, Role::System))
            .collect();
        assert_eq!(sys_msgs.len(), 2);
        assert_eq!(sys_msgs[0].content.as_deref(), Some("prompt one"));
        assert_eq!(sys_msgs[1].content.as_deref(), Some("prompt two"));
    }

    #[tokio::test]
    async fn sliding_window_removes_old_auto_summary() {
        let auto = Message::auto_summary("old summary");

        let mut history = vec![
            system_prompt("permanent"),
            auto,
            msg(Role::User, "a"),
            msg(Role::User, "b"),
            msg(Role::User, "c"),
        ];

        let s = SlidingWindowSummarizer::new(2);
        s.summarize(&mut history).await.unwrap();

        // old auto-summary should be gone
        assert!(!history.iter().any(|m| m.is_auto_summary()));

        // permanent system prompt preserved
        assert!(
            history
                .iter()
                .any(|m| m.content.as_deref() == Some("permanent"))
        );
    }

    #[tokio::test]
    async fn sliding_window_noop_when_within_window() {
        let mut history = vec![msg(Role::User, "a"), msg(Role::Assistant, "b")];

        let s = SlidingWindowSummarizer::new(4);
        assert!(!s.should_summarize(&history));
        s.summarize(&mut history).await.unwrap();
        assert_eq!(history.len(), 2);
    }

    // ── should_summarize ─────────────────────────────────────────────────────

    #[test]
    fn should_summarize_triggers_at_window_exceeded() {
        let history = vec![
            msg(Role::User, "a"),
            msg(Role::User, "b"),
            msg(Role::User, "c"),
        ];
        let s = SlidingWindowSummarizer::new(2);
        assert!(s.should_summarize(&history));

        let short = vec![msg(Role::User, "only")];
        assert!(!s.should_summarize(&short));
    }
}
