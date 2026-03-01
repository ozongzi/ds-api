# Changelog

All notable changes to this project will be documented in this file.

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
