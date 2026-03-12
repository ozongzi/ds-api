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
ds-api  = "0.8.0"
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

---

## Injecting messages mid-run

You can send a message into a running agent loop — useful when the user types something while the agent is still executing tools.

The interrupt channel is attached with `.with_interrupt_channel()` and returns the agent plus a sender you can use from any task. The sender type (`InterruptSender`) is a re-export of `tokio::sync::mpsc::UnboundedSender<String>`, so it is cheap to clone and use concurrently:

```rust
let (agent, tx) = DeepseekAgent::new(token)
    .with_streaming()
    .add_tool(SlowTool)
    .with_interrupt_channel();
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

## Contributing

PRs welcome. Keep changes focused; update public API docs when behaviour changes.

## License

MIT OR Apache-2.0
