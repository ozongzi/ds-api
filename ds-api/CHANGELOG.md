# Changelog

All notable changes to this project will be documented in this file.

## [0.5.0] - 2026-03-08

### Summary
Breaking release: the agent event type has been redesigned from a flat struct to a proper enum.

### Breaking changes

**Agent event type**
- `AgentResponse` struct removed. Replaced by `AgentEvent` enum.
- `ToolCallEvent` struct removed. Replaced by two focused structs:
  - `ToolCallInfo` — carries `id`, `name`, `args`; yielded before execution.
  - `ToolCallResult` — carries `id`, `name`, `args`, `result`; yielded after execution.
- `AgentStream` now implements `Stream<Item = Result<AgentEvent, ApiError>>`.
- Tool call previews and results are now yielded **one per event** (previously batched in a `Vec`).
- The old `tc.result == Value::Null` idiom for distinguishing previews from results is gone; the variant itself encodes the distinction.

**Summarizer**
- `TokenBasedSummarizer` removed.
- `Summarizer::summarize` is now `async` (returns `Pin<Box<dyn Future<Output = Result<(), ApiError>>>>`). Any custom `Summarizer` implementation must be updated.
- New default summarizer is `LlmSummarizer`, which calls DeepSeek to produce a semantic summary of older turns. It requires an `ApiClient` at construction time.
- New alternative `SlidingWindowSummarizer` replaces `TokenBasedSummarizer` for cases where zero extra API calls are desired.
- Permanent `Role::System` messages set via `with_system_prompt` are now protected and never removed by any built-in summarizer.

### Migration

Replace:
```rust
// old
use ds_api::{AgentResponse, ToolCallEvent};

while let Some(event) = stream.next().await {
    let ev = event?;
    if let Some(text) = ev.content {
        print!("{text}");
    }
    for tc in ev.tool_calls {
        if tc.result.is_null() {
            println!("[calling {}({})]", tc.name, tc.args);
        } else {
            println!("[result] {}", tc.result);
        }
    }
}
```
with:
```rust
// new
use ds_api::AgentEvent;

while let Some(event) = stream.next().await {
    match event? {
        AgentEvent::Token(text)    => print!("{text}"),
        AgentEvent::ToolCall(c)    => println!("[calling {}({})]", c.name, c.args),
        AgentEvent::ToolResult(r)  => println!("[result] {}", r.result),
    }
}
```

### Notes
- The `AgentEvent::Token` variant carries assistant text in both streaming and non-streaming modes. In streaming mode each `Token` is a single SSE delta; in non-streaming mode the full response text arrives as one `Token`.
- `ToolCall` and `ToolResult` events are emitted in matching order (first call → first result).
- `LlmSummarizer` errors (e.g. a transient API failure during summarization) are swallowed silently so an ongoing conversation is never aborted by a failed summary attempt.
- `SlidingWindowSummarizer` takes a `window: usize` argument and never makes an API call.

**Architecture**
- `DeepseekConversation` renamed to `Conversation`. The `Conversation` trait has been removed — there is now a single concrete struct with all methods defined directly on it.
- `DeepseekAgent` no longer holds a redundant `client` field; the single `ApiClient` lives inside `Conversation`.
- `AgentStream` state machine simplified: the `YieldingToolCalls` and `YieldingToolResults` states now carry their own queues (`VecDeque`) instead of storing them as loose fields on the stream struct. This makes the state machine self-contained and eliminates implicit field–state coupling.
- The `[auto-summary]` magic string is now centralised as `Message::AUTO_SUMMARY_TAG`, with `Message::is_auto_summary()` and `Message::auto_summary()` helpers. Custom `Summarizer` implementations should use these instead of comparing name strings directly.

### Migration — Architecture

Replace:
```rust
// old
use ds_api::DeepseekConversation;
let conv = DeepseekConversation::new(client);
```
with:
```rust
// new
use ds_api::Conversation;
let conv = Conversation::new(client);
```

### Migration — Summarizer

Replace:
```rust
// old
use ds_api::TokenBasedSummarizer;

agent.with_summarizer(TokenBasedSummarizer {
    threshold: 60_000,
    retain_last: 10,
    ..Default::default()
})
```
with one of:
```rust
// new — semantic LLM summary (default)
use ds_api::{ApiClient, LlmSummarizer};

agent.with_summarizer(
    LlmSummarizer::new(ApiClient::new(&token))
        .token_threshold(60_000)
        .retain_last(10),
)
```
```rust
// new — sliding window, no extra API calls
use ds_api::SlidingWindowSummarizer;

agent.with_summarizer(SlidingWindowSummarizer::new(20))
```

## [0.3.2] - 2026-03-01

### Summary
This is a patch release that improves the token estimation heuristic, updates documentation and examples, and bumps the crate version to `0.3.2`.

### Changes
- Bumped crate version to `0.3.2`.
- Improved token estimation:
  - Adjusted the chars-to-token heuristic to better handle multibyte characters and edge cases.
  - Fixed an off-by-one rounding issue in the estimator.
- Documentation updates:
  - Updated README and release notes to mention the token estimator improvement and version bump.
  - Ensured examples reference the correct behavior and version.
- Packaging:
  - `ds-api/Cargo.toml` version updated to `0.3.2`.

### Notes
- This release contains no public API changes; it is safe for downstream users (semver patch).
- Recommended checks before publishing:
  - `cargo test --manifest-path ds-api/Cargo.toml`
  - `cargo clippy -p ds-api -- -D warnings`
  - `cargo package --manifest-path ds-api/Cargo.toml`


## [0.3.0] - 2026-02-28

### Summary
This release is a refactor-and-improve release that focuses on:
- Modularization and code hygiene (split large modules into focused submodules).
- English documentation and doc-comments across the crate.
- Observability: tracing instrumentation added to critical API paths.
- Usable examples: runnable example(s) in `examples/` that demonstrate agent + tool flows.
- Linting and tests: Clippy warnings resolved and unit + doctests passing.

### Highlights
- Refactor: Split large modules into smaller submodules for `api/`, `agent/`, `conversation/`, and `raw/`.
- Docs: Translated remaining Chinese inline comments and Rustdoc comments to English across `src/`.
- Examples: Added a runnable `examples/agent_demo.rs` that demonstrates registering a tool and streaming agent events.
- Observability: Added structured tracing calls to `ApiClient` critical paths (request send, streaming, parsing).
- Linting: Clippy issues addressed; the repository compiles cleanly under `-D warnings`.
- Tests: All existing unit tests and doctests pass locally.

### Breaking changes
This release includes intentional breaking changes from earlier 0.x versions:
- `Request` and `DeepseekClient` were removed. Use `ApiRequest` and `ApiClient` instead.
- `NormalChatter` and `SimpleChatter` have been removed. Use `DeepseekConversation` and `DeepseekAgent`.
- The `Model` enum is no longer exported as a top-level public type. Use `ApiRequest::deepseek_chat(...)` or `ApiRequest::deepseek_reasoner(...)` to choose a model.
- Public signatures for some types were reorganized through module splitting; consumer code that referenced internal file paths may need to adjust imports to the new layout.

See "Migration notes" below for examples.

### Migration notes
- Replace:
```rust
// old
let req = Request::basic_query(...);
let client = DeepseekClient::new(token);
```
with:
```rust
use ds_api::{ApiClient, ApiRequest};
let client = ApiClient::new(token);
let req = ApiRequest::deepseek_chat(messages).max_tokens(150);
let resp = client.send(req).await?;
```

- Replace:
```rust
// old
let chatter = NormalChatter::new(...);
```
with:
```rust
use ds_api::DeepseekConversation;
let conv = DeepseekConversation::new(client.clone());
```

- Tools:
  - Tools are declared with the `#[tool]` macro and implement the `Tool` trait. Register with `DeepseekAgent::add_tool`.
  - Agent streaming yields two-phase `AgentResponse` events: first preview (assistant content + tool call requests), then the tool results.

### Observability / logging
- `tracing` and `tracing-subscriber` added as optional dependencies to enable structured logging.
- `ApiClient` emits spans and events for:
  - request start (URL + method),
  - applied timeout,
  - HTTP response receive,
  - stream connection and chunk parsing,
  - JSON parsing errors and non-success responses.
- NOTE: Library does NOT automatically install a global tracing subscriber. App binaries/examples should initialize a subscriber (for example: `tracing_subscriber::fmt::init()` or a configured subscriber).

### Examples
- `examples/agent_demo.rs` — runnable demonstration of an agent registering a `WeatherTool` and consuming the agent stream. Run:
```bash
cargo run --example agent_demo --manifest-path ds-api/Cargo.toml
```
Ensure `DEEPSEEK_API_KEY` is set in your environment.

### Packaging / publishing notes
- Before publishing:
  - Run `cargo test --manifest-path ds-api/Cargo.toml`.
  - Run `cargo clippy --all-targets --all-features -- -D warnings`.
  - Run `cargo package --manifest-path ds-api/Cargo.toml` to verify packaging.
  - Optionally run `cargo publish --manifest-path ds-api/Cargo.toml --dry-run`.

### Internal changes / developer notes
- `src/raw` reorganized into `request/` and `response/` submodules with doctests fixed.
- `agent` split into `agent_core.rs` and `stream.rs` (stream state machine logic isolated).
- `api` reorganized into `client.rs` and `request.rs`.
- Removed obsolete files and renamed `tool.rs` → `tool_trait.rs` (public trait export preserved).
- Many internal imports and visibility specifiers adjusted; public re-exports in `lib.rs` are kept to reduce churn for users.

### Tests
- Unit tests and doctests were updated/moved with the refactor and verify:
  - serialization/ deserialization of `raw` types,
  - conversation/summarizer unit tests,
  - basic agent flow tests.
- All tests pass locally at the time of preparing this release notes.

### Contributors
- Core maintainer: ozongzi
- Thanks to contributors who helped with refactor, translations, examples, and Lint fixes.

---

If you want, I can:
- Prepare and commit `CHANGELOG.md` (this file) and bump `version = "0.3.0"` in `ds-api/Cargo.toml`.
- Create `v0.3.0` annotated git tag and push it.
- Run `cargo publish --dry-run` (or a real `cargo publish`) — but that requires your confirmation and proper crates.io credentials.
