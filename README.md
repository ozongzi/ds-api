# ds-api

[![crates.io](https://img.shields.io/crates/v/ds-api.svg)](https://crates.io/crates/ds-api)
[![docs.rs](https://img.shields.io/docsrs/ds-api)](https://docs.rs/ds-api)
[![license](https://img.shields.io/crates/l/ds-api.svg)](https://github.com/ozongzi/ds-api/blob/main/LICENSE-MIT)

A Rust SDK for building LLM agents on top of DeepSeek (and any OpenAI-compatible API). Define tools in plain Rust, plug them into an agent, and consume a stream of events as the model thinks, calls tools, and responds.

---

## Quickstart

Set your API key and add the dependency:

```bash
export DEEPSEEK_API_KEY="sk-..."
```

```toml
# Cargo.toml
[dependencies]
ds-api  = "0.10.3"
futures = "0.3"
tokio   = { version = "1", features = ["full"] }
serde   = { version = "1", features = ["derive"] }
```

```rust
use ds_api::{AgentEvent, DeepseekAgent, tool};
use futures::StreamExt;
use serde_json::{Value, json};

struct Search;

#[tool]
impl ds_api::Tool for Search {
    /// Search the web and return results.
    /// query: the search query
    async fn search(&self, query: String) -> Value {
        json!({ "results": format!("results for: {query}") })
    }
}

#[tokio::main]
async fn main() {
    let token = std::env::var("DEEPSEEK_API_KEY").unwrap();

    let mut stream = DeepseekAgent::new(token)
        .add_tool(Search)
        .chat("What's the latest news about Rust?");

    while let Some(event) = stream.next().await {
        match event.unwrap() {
            AgentEvent::Token(text)       => print!("{text}"),
            AgentEvent::ToolCall(c)       => println!("\n[calling {}]", c.name),
            AgentEvent::ToolResult(r)     => println!("[result] {}", r.result),
            AgentEvent::ReasoningToken(t) => print!("{t}"),
        }
    }
}
```

The agent runs the full loop for you: it calls the model, dispatches any tool calls, feeds the results back, and keeps going until the model stops requesting tools.

---

## Defining tools

Annotate an `impl Tool for YourStruct` block with `#[tool]`. Each method becomes a callable tool:

- **Doc comment on the impl block** → tool description
- **`/// param: description`** lines in each method's doc comment → argument descriptions
- Return type just needs to be `serde::Serialize` — the macro handles the JSON schema

```rust
use ds_api::tool;
use serde_json::{Value, json};

struct Calculator;

#[tool]
impl ds_api::Tool for Calculator {
    /// Add two numbers together.
    /// a: first number
    /// b: second number
    async fn add(&self, a: f64, b: f64) -> Value {
        json!({ "result": a + b })
    }

    /// Multiply two numbers.
    /// a: first number
    /// b: second number
    async fn multiply(&self, a: f64, b: f64) -> Value {
        json!({ "result": a * b })
    }
}
```

One struct can have multiple methods — they register as separate tools. Stack as many tools as you need with `.add_tool(...)`.

---

## Streaming

Call `.with_streaming()` to get token-by-token output instead of waiting for the full response:

```rust
let mut stream = DeepseekAgent::new(token)
    .with_streaming()
    .add_tool(Search)
    .chat("Search for something and summarise it");

while let Some(event) = stream.next().await {
    match event.unwrap() {
        AgentEvent::Token(t)      => { print!("{t}"); io::stdout().flush().ok(); }
        AgentEvent::ToolCall(c)   => {
            // In streaming mode, ToolCall fires once per SSE chunk.
            // First chunk: c.delta is empty, c.name is set — good moment to show "calling X".
            // Subsequent chunks: c.delta contains incremental argument JSON.
            // In non-streaming mode, exactly one ToolCall fires with the full args in c.delta.
            if c.delta.is_empty() { println!("\n[calling {}]", c.name); }
        }
        AgentEvent::ToolResult(r) => println!("[done] {}: {}", r.name, r.result),
        _                         => {}
    }
}
```

### AgentEvent reference

| Variant | When | Notes |
|---------|------|-------|
| `Token(String)` | Model is speaking | Streaming: one fragment per chunk. Non-streaming: whole reply at once. |
| `ReasoningToken(String)` | Model is thinking | Only from reasoning models (e.g. `deepseek-reasoner`). |
| `ToolCall(ToolCallChunk)` | Tool call in progress | `chunk.id`, `chunk.name`, `chunk.delta`. Streaming: multiple per call. Non-streaming: one per call. |
| `ToolResult(ToolCallResult)` | Tool finished | `result.name`, `result.args`, `result.result`. |

---

## Using a different model or provider

Any OpenAI-compatible endpoint works:

```rust
// OpenRouter
let agent = DeepseekAgent::custom(
    "sk-or-...",
    "https://openrouter.ai/api/v1",
    "meta-llama/llama-3.3-70b-instruct:free",
);

// deepseek-reasoner (think before responding)
let agent = DeepseekAgent::new(token)
    .with_model("deepseek-reasoner");
```

## Custom top-level request fields (`extra_body`)

The library exposes an `extra_body` mechanism to let you merge arbitrary top-level JSON fields into the HTTP request body sent to the provider. This is useful for passing provider-specific or experimental options that are not (yet) modelled by the typed request structure.

There are two primary places you can attach `extra_body` fields:

- On an `ApiRequest` (fine-grained, request-local)
- On a `DeepseekAgent` (convenient builder-style; merged into the next requests built from the agent)

Important notes
- Fields in `extra_body` are flattened into the top-level JSON via `serde(flatten)`, so they appear as peers to `messages`, `model`, etc.
- Avoid key collisions with existing top-level names (e.g. `messages`, `model`). The intended usage is adding provider-specific keys.
- Agent-held `extra_body` maps are merged into the `ApiRequest` when the request is built. (If you want per-request control prefer `ApiRequest::extra_body`.)

Examples

- Using `ApiRequest`:

```rust
use serde_json::{Map, json};
use ds_api::ApiRequest;

let mut m = Map::new();
m.insert("my_flag".to_string(), json!(true));

let req = ApiRequest::builder()
    .messages(vec![])
    .extra_body(m);

// send via ApiClient, or use within library internals that accept ApiRequest
```

- Using `DeepseekAgent` builder helpers:

```rust
use serde_json::{Map, json};
use ds_api::DeepseekAgent;

let mut m = Map::new();
m.insert("provider_option".to_string(), json!("value"));

let agent = DeepseekAgent::new(token)
    .extra_body(m)              // merge these fields into subsequent requests
    .chat("Hello world");
```

- Convenience single-field helper:

```rust
let agent = DeepseekAgent::new(token)
    .extra_field("provider_option", serde_json::json!("value"));
```

Adding a unit test (suggested)
To verify serialization behaviour you can add a unit test that constructs a `ChatCompletionRequest` with `extra_body` and confirms the resulting top-level JSON contains the custom keys. Example test you can add to `ds-api/src/raw/request/chat_completion.rs` (or place in an integration test under `ds-api/tests/`):

```rust
#[test]
fn test_extra_body_serialize_merge() {
    use ds_api::raw::request::ChatCompletionRequest;
    use ds_api::raw::model::Model;
    use serde_json::{json, Map, Value};

    // Build an extra map
    let mut extra = Map::<String, Value>::new();
    extra.insert("x_custom".to_string(), json!("v1"));
    extra.insert("x_flag".to_string(), json!(true));

    // Create a request with extra_body set
    let req = ChatCompletionRequest {
        messages: vec![],
        model: Model::DeepseekChat,
        extra_body: Some(extra),
        ..Default::default()
    };

    // Serialize to a Value and assert the custom keys are present at top-level
    let s = serde_json::to_value(&req).expect("serialize");
    assert_eq!(s.get("x_custom").and_then(|v| v.as_str()).unwrap(), "v1");
    assert_eq!(s.get("x_flag").and_then(|v| v.as_bool()).unwrap(), true);
}
```

Run tests:
- From repository root run `cargo test -p ds-api` to run the crate's tests (or `cargo test` for workspace-wide).

This README section documents the intended `extra_body` usage and supplies a test template you can drop into the codebase to assert the top-level merging behaviour.


Any OpenAI-compatible endpoint works:

```rust
// OpenRouter
let agent = DeepseekAgent::custom(
    "sk-or-...",
    "https://openrouter.ai/api/v1",
    "meta-llama/llama-3.3-70b-instruct:free",
);

// deepseek-reasoner (think before responding)
let agent = DeepseekAgent::new(token)
    .with_model("deepseek-reasoner");
```

---

## Injecting messages mid-run

You can send a message into a running agent loop — useful when the user types something while the agent is still executing tools.

The interrupt channel is attached with `.with_interrupt_channel()` and returns the agent plus a sender you can use from any task. The sender type (`InterruptSender`) is a re-export of `tokio::sync::mpsc::UnboundedSender<String>`, so it is cheap to clone and use concurrently:

```rust
let agent = DeepseekAgent::new(token)
    .with_streaming()
    .add_tool(SlowTool)

let tx = agent.interrupt_sender();
```

Behavior and semantics
- Sending an interrupt: call `tx.send("...".into()).unwrap()` from any task or callback. The message will be delivered into the agent's conversation history.
- During tool execution: the agent now actively listens for interrupts while a tool is running. If an interrupt message arrives while a tool is executing, the executor will:
  1. Immediately append the interrupt text to the conversation history as a `Role::User` message (and drain any queued interrupt messages in order).
  2. Abort the currently running tool (the tool future is cancelled) and stop executing further tools for the current round. (can close only when the tool is awaiting)
  3. Record a placeholder result for the aborted tool (the runtime exposes this as an error-shaped JSON result), and then proceed to the next API turn so the model sees the injected user message.
- Between turns / idle transition: any queued interrupts are drained before the next API call so injected messages are always visible to the model on the next turn.

Example: cancel a running tool and pivot
```rust
// Start the agent and get an interrupt sender.
let (agent, tx) = DeepseekAgent::new(token)
    .with_streaming()
    .add_tool(SlowTool)
    .with_interrupt_channel();

// In another task (e.g. user action), send an interrupt to change the plan.
tx.send("Actually, cancel that and do X instead.".into()).unwrap();

// If the agent is currently executing a tool, that tool will be aborted and the
// interrupt will be pushed into history so the next API turn sees it.
let mut stream = agent.chat("Do the slow thing.");
```

Notes
- `InterruptSender` is non-blocking and can be cloned; use it from any async context without awaiting.
- Aborting a tool is implemented by cancelling the tool future (via the runtime). This is effective for most async tools, but if a tool holds on to external, non-cancellable resources you may want to implement cooperative cancellation inside the tool (for example, by checking a cancellation token).
- The agent ensures interrupt message ordering by draining remaining queued interrupt messages when an interrupt is observed.

---

## MCP tools

MCP (Model Context Protocol) lets you use external processes as tools — Node scripts, Python services, anything that speaks MCP over stdio:

```rust
// Requires the `mcp` feature
let agent = DeepseekAgent::new(token)
    .add_tool(McpTool::stdio("npx", &["-y", "@playwright/mcp"]).await?);
```

---

## Exposing tools as an MCP server

The `mcp-server` feature lets you turn any `ToolBundle` into a standalone MCP server so other LLM clients (Claude Desktop, MCP Studio, etc.) can call your Rust tools.

```toml
[dependencies]
ds-api = { version = "0.10", features = ["mcp-server"] }
tokio  = { version = "1", features = ["full"] }
```

### Stdio mode (Claude Desktop / MCP Studio)

Add this binary to your project and point Claude Desktop at it:

```rust
use ds_api::{McpServer, ToolBundle, tool};

struct Calculator;

#[tool]
impl ds_api::Tool for Calculator {
    /// Add two numbers.
    /// a: first operand
    /// b: second operand
    async fn add(&self, a: f64, b: f64) -> f64 { a + b }

    /// Multiply two numbers.
    /// a: first operand
    /// b: second operand
    async fn multiply(&self, a: f64, b: f64) -> f64 { a * b }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    McpServer::new(ToolBundle::new().add(Calculator))
        .with_name("my-calc-server")
        .serve_stdio()
        .await?;
    Ok(())
}
```

Register it in `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "my-calc": {
      "command": "/path/to/your/binary"
    }
  }
}
```

### HTTP mode (Streamable HTTP transport)

```rust
use ds_api::{McpServer, ToolBundle, tool};

struct Search;

#[tool]
impl ds_api::Tool for Search {
    /// Search the web.
    /// query: what to search for
    async fn search(&self, query: String) -> String {
        format!("results for: {query}")
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // MCP endpoint available at POST /mcp
    McpServer::new(ToolBundle::new().add(Search))
        .serve_http("0.0.0.0:3000")
        .await?;
    Ok(())
}
```

### Custom routing

For custom Axum routing, use `into_http_service()` to get a Tower-compatible service:

```rust
use ds_api::{McpServer, ToolBundle};
use rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig;

let service = McpServer::new(ToolBundle::new().add(MyTools))
    .into_http_service(Default::default());

let router = axum::Router::new()
    .nest_service("/mcp", service)
    .route("/health", axum::routing::get(|| async { "ok" }));

let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
axum::serve(listener, router).await?;
```

---

## Tool Bundle

ToolBundle can handle multiple Tool implementations and
builds a name->index map for dispatch.

### Example

```rust
let group = ToolBundle::new()
    .add(FileSpells)
    .add(SearchSpells)
    .add(ShellSpells);

let agent = DeepseekAgent::custom(...)
    .add_tool(group)
    .add_tool(UiSpells { ... })
    .add_tool(SpawnSpell { ... });
```

---

## Familiar
Familiar is a high-level agent built on top of ds-api. It provides opinionated defaults and a batteries-included experience for common agent patterns. Check out [familiar](https://github.com/ozongzi/familiar)

---

## Contributing

PRs welcome. Keep changes focused; update public API docs when behaviour changes.

## License

MIT OR Apache-2.0
