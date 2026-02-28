ds-api-workspace/ds-api/README.md
# ds-api — Rust client for DeepSeek (refactored)

This crate provides a layered, ergonomic Rust client for the DeepSeek chat API.

High-level design
- raw — low-level types that mirror the API JSON (kept under `ds_api::raw` but not recommended).
- api — safe, chainable request builder and HTTP client (`ApiRequest`, `ApiClient`).
- conversation — session management and summarization (`DeepseekConversation`, `Summarizer`).
- agent — agent orchestration with tools and auto-summary (`DeepseekAgent`).

This README documents the new API, breaking changes, migration steps, and examples.

## Quick highlights (new API)

- ApiRequest — chainable, safe builder. Use `ApiRequest::builder()` or `ApiRequest::deepseek_chat(...)` / `ApiRequest::deepseek_reasoner(...)` to choose a model (Model enum is intentionally not exported).
- ApiClient — owns token/base_url/reqwest::Client; send blocking or streaming requests.
- DeepseekConversation — manages history and auto-summary via the `Summarizer` trait. Default summarizer is `TokenBasedSummarizer`.
- DeepseekAgent — high-level agent that combines a conversation, tools (via `tool` macro), and auto summary. Agent yields two-step events for tool calls: first a preview (content + tool call requests) then tool results.
- raw module still available under `ds_api::raw` for advanced users, but not part of the primary recommended API.

## Breaking changes (important)

This refactor intentionally removes compatibility with older API shapes. Notable breaking items:

- `Request` and `DeepseekClient` (previous high-level types) have been removed. Use `ApiRequest` and `ApiClient` instead.
- `NormalChatter` and `SimpleChatter` were removed. Use `DeepseekConversation` and `DeepseekAgent`.
- `Model` enum is no longer exported directly as part of the public API. Use `ApiRequest::deepseek_chat(...)` or `ApiRequest::deepseek_reasoner(...)` to choose a model.
- Unsafe raw accessors (e.g. `from_raw_unchecked`, `get_raw_mut`) have been removed from the public API.
- Summarization is now pluggable via the `Summarizer` trait. Default `TokenBasedSummarizer` skips `system` messages when estimating tokens and triggers at the default threshold (100_000 tokens estimate).

If you're migrating from the old crate:
- Replace `Request` -> `ApiRequest`
- Replace `DeepseekClient` -> `ApiClient`
- Replace `NormalChatter` -> `DeepseekConversation` (or `DeepseekAgent` if you need tools)
- Replace `SimpleChatter` -> `DeepseekConversation` thin wrapper as needed

## Example: simple non-streaming request

```ds-api-workspace/ds-api/examples/agent_demo.rs#L1-40
// Build an ApiRequest and send a non-streaming call.
use ds_api::{ApiClient, ApiRequest};
use ds_api::raw::request::message::Message;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = std::env::var("DEEPSEEK_API_KEY")?;
    let client = ApiClient::new(token);

    let req = ApiRequest::deepseek_chat(vec![
        Message::new(ds_api::raw::request::message::Role::User, "Hello from Rust"),
    ])
    .max_tokens(150)
    .json();

    let resp = client.send(req).await?;
    println!("Response content: {}", resp.content()?);
    Ok(())
}
```

## Example: streaming text from ApiClient

```ds-api-workspace/ds-api/examples/agent_demo.rs#L40-120
use ds_api::{ApiClient, ApiRequest};
use futures::StreamExt;

let token = std::env::var("DEEPSEEK_API_KEY")?;
let client = ApiClient::new(token);

let req = ApiRequest::deepseek_chat(vec![Message::new(Role::User, "Tell me a story")])
    .stream(true);

let mut stream = client.stream_text(req).await?;
while let Some(chunk_res) = stream.next().await {
    match chunk_res {
        Ok(text) => print!("{}", text),
        Err(err) => eprintln!("Stream error: {}", err),
    }
}
```

## Example: DeepseekConversation (auto summary)

```ds-api-workspace/ds-api/examples/agent_demo.rs#L120-200
use ds_api::{ApiClient, ApiRequest, DeepseekConversation, Message, Role};

let token = std::env::var("DEEPSEEK_API_KEY")?;
let client = ApiClient::new(token);

let mut conv = DeepseekConversation::new(client.clone())
    .with_history(vec![Message::new(Role::System, "You are a helpful assistant.")]);

conv.push_user_input("Hello! Tell me about Rust.".to_string());
let reply = conv.send_once().await?;
println!("Assistant: {:?}", reply);
```

## Example: DeepseekAgent with tools (preferred flow)

- Agent yields two-phase events when the model triggers tool calls:
  1. First yield: assistant `content` paired with `tool_calls` preview (result is `null`).
  2. Second yield: tool execution results (and these results are appended to conversation history as `Role::Tool` messages).

Tool functions are declared with the `#[tool]` macro.

```ds-api-workspace/ds-api/examples/agent_demo.rs#L1-200
use ds_api::{DeepseekAgent, tool};
use futures::StreamExt;
use serde_json::json;

struct WeatherTool {
    client: reqwest::Client,
}

#[tool]
impl Tool for WeatherTool {
    /// Get weather for a city
    async fn get_weather(&self, city: String, _unit: Option<String>) -> serde_json::Value {
        let url = format!("https://wttr.in/{}?format=3", city);
        let text = self.client.get(&url).send().await
            .and_then(|r| r.text().await)
            .unwrap_or_else(|e| e.to_string());
        json!({ "city": city, "weather": text })
    }
}

#[tokio::main]
async fn main() {
    let token = std::env::var("DEEPSEEK_API_KEY").unwrap();
    let agent = DeepseekAgent::new(token)
        .add_tool(WeatherTool { client: reqwest::Client::new() })
        .with_system_prompt("You are an assistant that can call tools.");

    let mut s = agent.chat("What's the weather in Beijing and Shanghai?");
    while let Some(event) = s.next().await {
        if let Some(content) = &event.content {
            println!("Assistant: {}", content);
        }
        for tc in &event.tool_calls {
            println!("Tool call preview/result: {} {} -> {}", tc.name, tc.args, tc.result);
        }
    }
}
```

## Summarizer and auto-summary behavior

- `Summarizer` trait: pluggable abstraction for summarization.
- Default: `TokenBasedSummarizer`:
  - Rough token estimate uses `chars / 4`.
  - SKIPS `system` messages when computing the estimate.
  - Default threshold: 100_000 estimated tokens (configurable).
  - When triggered, older messages are summarized into a single `system` message (prefixed/marked with `[auto-summary]` in `name`).

You can implement `Summarizer` to call an LLM for semantic summaries or to use a different heuristic.

## Migration guidance

- Replace previous usage of `Request::basic_query(...)` with `ApiRequest::deepseek_chat(...)` or `ApiRequest::builder()`.
- Use `ApiClient::new(token)` instead of `DeepseekClient`.
- Replace `NormalChatter` and `SimpleChatter` with `DeepseekConversation` or `DeepseekAgent`.
- If you previously relied on raw accessors, migrate carefully — raw module still exists at `ds_api::raw` but the recommended approach is through `ApiRequest` and `ApiClient`.

## Project layout (high level)

- `src/raw` — raw request/response types (kept for advanced use).
- `src/api.rs` — `ApiRequest`, `ApiClient`.
- `src/conversation/mod.rs` — `DeepseekConversation`, `Summarizer` + default summarizer.
- `src/agent.rs` — `DeepseekAgent` and streaming agent orchestration.
- `src/tool.rs` + `ds-api-macros` crate — tooling macro and Tool trait.

## Tests, linting, and build
- The project uses `cargo` for builds, and Clippy is enforced. After refactor we run `cargo clippy -p ds-api -- -D warnings` as part of CI.
- Run:
```ds-api-workspace/ds-api/README.md#L1-1
# in repo root
cargo build
cargo test
cargo clippy -p ds-api -- -D warnings
```

## Contributing & Release notes

- This refactor is breaking by design. A migration guide is in this README (above).
- If you maintain downstream consumers, notify them about:
  - Removed `Request`/`DeepseekClient`, `NormalChatter`, `SimpleChatter`.
  - New preferred entry points: `ApiRequest`, `ApiClient`, `DeepseekConversation`, `DeepseekAgent`.
- Future work:
  - Provide thin compatibility wrappers (if required).
  - Improve summarizer with a semantic LLM-backed default option (configurable).

## License
Check `Cargo.toml` for license information.

---

If you want, I can:
- Produce a dedicated `UPGRADING.md` with automated migration diffs.
- Replace the example files in `examples/` with simpler, up-to-date usage snippets.
- Add a short changelog entry under `docs/` describing the refactor and linking to migration tips.

Which would you prefer next?
