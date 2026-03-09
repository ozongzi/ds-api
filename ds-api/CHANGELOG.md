# Changelog

All notable changes to this project will be documented in this file.

## [0.5.6] - 2026-03-10

### Summary

New `mcp` optional feature: connect any MCP server's tools to `DeepseekAgent` with a single dependency line.

### New features

**`McpTool` — MCP client support (`features = ["mcp"]`)**

- New `ds_api::McpTool` implements the `Tool` trait and can be passed directly to `DeepseekAgent::add_tool()`.
- Two transports supported:
  - `McpTool::stdio(program, args)` — spawns a child process (`npx`, `uvx`, or any binary) and communicates over stdin/stdout.
  - `McpTool::http(url)` — connects to a remote MCP server over Streamable HTTP.
- At construction time, `tools/list` is called automatically (pagination handled transparently) and the tool list is cached. Each subsequent model tool call is forwarded via `tools/call`.
- The MCP server's `inputSchema` is passed through as-is to the DeepSeek API `parameters` field — no manual schema configuration needed.
- New `ds_api::mcp::McpError` error type covering process spawn failure, handshake failure, tool list fetch failure, and tool call failure.

**Usage**

```toml
[dependencies]
ds-api = { version = "0.5", features = ["mcp"] }
```

```rust
use ds_api::{DeepseekAgent, McpTool};

let agent = DeepseekAgent::new(token)
    .add_tool(McpTool::stdio("npx", &["-y", "@playwright/mcp"]).await?)
    .add_tool(McpTool::stdio("uvx", &["mcp-server-git"]).await?)
    .add_tool(McpTool::http("https://mcp.example.com/").await?);
```

## [0.5.4] - 2026-03-09

### Summary
Bug fix: interrupt channel messages were silently dropped on turns where the model returned a plain text response with no tool calls.

### Bug fixes

**Interrupt channel now drained before every API turn**
- Previously, `with_interrupt_channel()` messages were only picked up inside `execute_tools()`, meaning any message sent during a no-tool turn was never inserted into the conversation history.
- `drain_interrupts()` is now also called at the top of the `Idle` state transition, so queued messages are always flushed before the next API call regardless of whether tools were used in the previous turn.

### Notes
- No breaking changes — all `0.5.x` code continues to compile unchanged.

---

## [0.5.3] - 2026-03-09

### Summary
Mid-loop user message injection via an interrupt channel. No breaking changes — all existing `0.5.x` code continues to compile unchanged.

### New features

**`DeepseekAgent::with_interrupt_channel()`**
- New builder method that attaches an `UnboundedSender<String>` to the agent.
- Returns `(DeepseekAgent, InterruptSender)` — the agent and the sender half of the channel.
- Any message sent through the `InterruptSender` is picked up automatically after the current tool-execution round finishes and appended to the conversation history as a `Role::User` message before the next API turn.
- The sender can be cloned freely and used from any task or callback (e.g. a Telegram bot handler) without blocking.
- If the agent is idle (not in a tool loop), messages accumulate in the channel and are drained on the next tool round.
- Agents without an interrupt channel (the default) are unaffected — no overhead.

```rust
let (agent, tx) = DeepseekAgent::new(token)
    .with_streaming()
    .add_tool(MyTool)
    .with_interrupt_channel();

// In another task — fires while the agent is executing tools:
tx.send("Actually, use Python instead.".into()).unwrap();
```

Timing — message is injected between tool round and next API turn:
```
User prompt
  → API call → ToolCall(search)
  → tool executing…  ← tx.send("change of plan") arrives here
  → ToolResult(search)
  → drain channel → push User("change of plan") into history
  → API call (model now sees the injected message)
  → Token("Sure, pivoting to…")
```

**`DeepseekAgent::history()`**
- New public read-only accessor returning `&[Message]` — the full conversation history in order.
- Includes system prompts, user turns, assistant replies, tool calls, tool results, and any auto-summary messages.
- Previously the `conversation` field was `pub(crate)` and inaccessible from application code.

```rust
for msg in agent.history() {
    println!("{:?}: {:?}", msg.role, msg.content);
}
```

**`InterruptSender` type alias**
- `ds_api::InterruptSender` is a re-export of `tokio::sync::mpsc::UnboundedSender<String>`.
- Import it directly instead of spelling out the full `tokio` path.

### New example

**`examples/interrupt.rs`**
- Demonstrates `with_interrupt_channel()` end-to-end.
- A `SlowCounter` tool counts to 5 with a 200 ms delay per step (~1 s total).
- A background task injects a follow-up message at 500 ms (mid-tool-execution).
- After the tool round finishes, the agent incorporates the injected message into its next reply.
- Shows `stream.into_agent()` recovery and `agent.history()` inspection.

Run with:
```bash
DEEPSEEK_API_KEY=sk-... cargo run --example interrupt
```

### Notes
- All tests pass.
- No breaking changes — `0.5.2` consumers require no code changes.

---

## [0.5.2] - 2026-03-10

### Summary
OpenAI-compatible provider support. No breaking changes — all existing `0.5.x` code continues to compile unchanged.

### New features

**`DeepseekAgent::custom(token, base_url, model)`**
- New constructor for pointing the agent at any OpenAI-compatible endpoint (OpenRouter, OpenAI, local Ollama, etc.).
- All three parameters are fixed at construction time; the agent is fully configured in one call.
- The default `LlmSummarizer` is automatically initialised with the same base URL and model — no manual `ApiClient` or `LlmSummarizer` wiring required.

```rust
// DeepSeek (unchanged)
let agent = DeepseekAgent::new(token);

// Any OpenAI-compatible provider
let agent = DeepseekAgent::custom(
    token,
    "https://openrouter.ai/api/v1",
    "meta-llama/llama-3.3-70b-instruct:free",
);
```

**`DeepseekAgent::with_model(model)` / `ApiRequest::with_model(model)` / `LlmSummarizer::with_model(model)`**
- New builder method on each type accepting any `impl Into<String>` model identifier.
- Removes the need to import or construct the internal `Model` enum for custom model names.

**`Model::Custom(String)` variant**
- The internal `Model` enum gained a `Custom(String)` variant with hand-written `Serialize`/`Deserialize` that passes the string through as-is.
- `Model` is not re-exported at the crate root; callers use the string-based builders above instead.

**`system_fingerprint` made optional in responses**
- `ChatCompletionResponse` and `ChatCompletionChunk` now deserialise correctly when the provider omits `system_fingerprint` (many non-DeepSeek providers do).

### Notes
- All 49 tests (22 unit, 10 integration, 17 doctest) pass.
- No breaking changes — `0.5.1` consumers require no code changes.
- The Roadmap item "OpenAI-compatible providers" is now complete.

---

## [0.5.1] - 2026-03-09

### Summary
Internal refactor: business logic extracted from the streaming state machine into a dedicated `executor` module.  No public API changes.

### Changes

**New `agent/executor.rs` module**
- Extracted all "do actual work" functions out of `stream.rs` into a new `executor.rs`:
  - `build_request` — assembles an `ApiRequest` from history + tools.
  - `run_summarize` — runs `maybe_summarize` and transfers agent ownership back.
  - `fetch_response` — non-streaming API call; appends assistant turn to history.
  - `connect_stream` — opens an SSE `BoxStream` for the current turn.
  - `execute_tools` — dispatches all pending tool calls and collects results.
  - `finalize_stream` — assembles complete `ToolCall` objects from SSE delta buffers and records the assistant turn.
  - `apply_chunk_delta` — applies one SSE chunk delta to the `StreamingData` accumulator.
  - `raw_to_tool_call_info` — converts a wire `ToolCall` to the public `ToolCallInfo` type.
- Internal accumulator types (`FetchResult`, `ToolsResult`, `PartialToolCall`, `StreamingData`) and future type aliases (`FetchFuture`, `ConnectFuture`, `ExecFuture`, `SummarizeFuture`) moved to `executor.rs`.
- `stream.rs` now contains only the `AgentStream` state machine and its `Stream` impl — no business logic, no `async fn`s.
- This separation makes it straightforward to add retries, timeouts, or parallel tool execution in the future without touching the state machine.

**`SlidingWindowSummarizer` improvements**
- Added `trigger_at(n: usize)` builder method: set the non-system message count above which summarization is triggered, independently of the `window` (retain count).  Useful when you want the window to only slide after a burst of messages rather than on every new message.
- `trigger_at` is silently clamped to `window + 1` if a value ≤ `window` is provided.
- Default behaviour is unchanged: triggers as soon as the non-system count exceeds `window`.

**Documentation**
- `AgentStream` now has a full doc comment with an example showing streaming event handling.
- `Summarizer` trait doc includes a complete custom-summarizer example (`TurnLimitSummarizer`).
- `AgentStreamState` variants have inline doc comments explaining each state's role.
- `executor.rs` functions all have doc comments explaining inputs, outputs, and side-effects.

### Notes
- All 36 existing tests (15 unit, 10 integration, 11 doctest) continue to pass.
- No breaking changes — `0.5.0` consumers require no code changes.

---

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
