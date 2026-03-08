# ds_api

**Your Rust functions. DeepSeek's brain. Zero glue code.**

```
cargo add ds_api
```

---

## The Problem

Building an LLM agent means writing a pile of code that has nothing to do with your actual problem:

- Hand-craft JSON schemas for every tool
- Parse and validate tool arguments from raw JSON
- Detect tool calls in the response
- Implement an agent loop that re-sends results to the model
- Wire up streaming yourself

Every project. Every time.

---

## The Solution

One macro. Your methods become AI tools.

```rust
use ds_api::{AgentEvent, DeepseekAgent, tool};
use futures::StreamExt;
use serde_json::{Value, json};

struct Search;

#[tool]
impl Tool for Search {
    /// Search the web for a query.
    /// query: the search query
    async fn search(&self, query: String) -> Value {
        json!({ "results": format!("top results for: {query}") })
    }
}

#[tokio::main]
async fn main() {
    let agent = DeepseekAgent::new(std::env::var("DEEPSEEK_API_KEY").unwrap())
        .add_tool(Search);

    let mut stream = agent.chat("What is Rust's ownership model?");

    while let Some(event) = stream.next().await {
        match event {
            Ok(AgentEvent::Token(text))   => print!("{text}"),
            Ok(AgentEvent::ToolCall(c))   => println!("\n[→ {}({})]", c.name, c.args),
            Ok(AgentEvent::ToolResult(r)) => println!("[✓ {}]", r.result),
            Err(e)                        => eprintln!("error: {e}"),
        }
    }
}
```

No schema. No argument parsing. No loop. Just your function.

---

## Key Features

### `#[tool]` — Zero-boilerplate tool registration

Annotate any `async fn`. The macro reads your doc comments, infers the JSON schema from your types, and registers everything automatically.

```rust
#[tool]
impl Tool for Database {
    /// Query the database and return matching rows.
    /// sql: SQL query to execute
    /// limit: maximum number of rows to return
    async fn query(&self, sql: String, limit: Option<u32>) -> Value {
        // your real implementation
    }
}
```

- **Doc comment → tool description.** No separate description field.
- **`param: description` in doc → parameter description.** Inline.
- **`Option<T>` → optional parameter.** The schema marks it non-required automatically.
- **Compile error on unsupported types.** You find out at build time, not runtime.

Supported types: `String`, `bool`, `f32/f64`, all integer primitives, `Vec<T>`, `Option<T>`.

---

### Typed event stream — `AgentEvent`

`chat()` returns a stream of strongly-typed events. The compiler forces you to handle every case.

```rust
match event? {
    AgentEvent::Token(text)    => /* assistant is typing    */,
    AgentEvent::ToolCall(c)    => /* model called a tool    */,
    AgentEvent::ToolResult(r)  => /* tool finished, here's r.result */,
}
```

No `if result.is_null()` hacks. No optional fields you have to remember to check. Each variant carries exactly what it means.

In streaming mode, `Token` arrives as SSE deltas. In non-streaming mode, it arrives as one chunk. Your match arm handles both.

---

### Automatic agent loop

The model requests a tool → `ds_api` executes it → feeds the result back → asks the model again. This continues until the model stops calling tools. You never write that loop.

```
User prompt
   └─▶ API call
         └─▶ ToolCall event (model wants data)
               └─▶ your function runs
                     └─▶ ToolResult event (result fed back)
                           └─▶ API call (model continues)
                                 └─▶ Token events (final answer)
```

---

### Context window management — automatic summarization

Long conversations are compressed automatically. The default summarizer (`LlmSummarizer`) calls DeepSeek to write a concise semantic summary of older turns, replaces them with a single system message, and keeps the most recent turns verbatim. Your `with_system_prompt` messages are never touched.

```rust
use ds_api::{LlmSummarizer, ApiClient};

// Default: trigger at ~60 000 estimated tokens, retain last 10 turns.
let agent = DeepseekAgent::new(&token)
    .with_summarizer(LlmSummarizer::new(ApiClient::new(&token)));

// Custom thresholds:
let agent = DeepseekAgent::new(&token)
    .with_summarizer(
        LlmSummarizer::new(ApiClient::new(&token))
            .token_threshold(40_000)
            .retain_last(6),
    );
```

If you prefer zero extra API calls, use `SlidingWindowSummarizer` instead — it keeps the last N turns and silently drops everything older:

```rust
use ds_api::SlidingWindowSummarizer;

let agent = DeepseekAgent::new(&token)
    .with_summarizer(SlidingWindowSummarizer::new(20));
```

Your agent stays within context limits without you counting tokens.

---

### Reusable agents — `into_agent()`

`chat()` consumes the agent to keep the borrow checker happy inside the async state machine. Get it back when the stream ends:

```rust
let mut agent = DeepseekAgent::new(token)
    .with_streaming()
    .add_tool(Shell);

loop {
    let mut stream = agent.chat(&prompt);
    while let Some(ev) = stream.next().await { /* ... */ }
    agent = stream.into_agent().unwrap(); // ← agent back, history intact
}
```

Full REPL with persistent conversation history. No cloning. No `Arc<Mutex<>>`.

---

## Real Example — Shell Agent

```rust
use ds_api::{AgentEvent, DeepseekAgent, tool};
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::process::Command;

struct Shell;

#[tool]
impl Tool for Shell {
    /// Execute a shell command and return stdout/stderr.
    /// command: the shell command to run
    async fn run(&self, command: String) -> Value {
        let out = Command::new("sh").arg("-c").arg(&command)
            .output().await.unwrap();
        json!({
            "stdout": String::from_utf8_lossy(&out.stdout),
            "stderr": String::from_utf8_lossy(&out.stderr),
            "status": out.status.code(),
        })
    }
}

#[tokio::main]
async fn main() {
    let mut agent = DeepseekAgent::new(std::env::var("DEEPSEEK_API_KEY").unwrap())
        .with_streaming()
        .with_system_prompt("You may run shell commands to answer questions.")
        .add_tool(Shell);

    loop {
        let mut line = String::new();
        std::io::stdin().read_line(&mut line).unwrap();

        let mut stream = agent.chat(line.trim());
        while let Some(ev) = stream.next().await {
            match ev {
                Ok(AgentEvent::Token(t))   => print!("{t}"),
                Ok(AgentEvent::ToolCall(c))   => println!("\n$ {}", c.args["command"].as_str().unwrap_or("")),
                Ok(AgentEvent::ToolResult(r)) => println!("{}", r.result["stdout"].as_str().unwrap_or("")),
                Err(e) => eprintln!("{e}"),
            }
        }
        agent = stream.into_agent().unwrap();
    }
}
```

The model decides when to call the shell. You just receive the events.

---

## What You Never Write

| Without ds_api | With ds_api |
|---|---|
| JSON schema per tool | `#[tool]` |
| Argument deserialization | automatic |
| Tool call detection | automatic |
| Agent loop | automatic |
| Token counting / context trimming | automatic |
| Streaming SSE wiring | automatic |

---

## Installation

```toml
[dependencies]
ds_api = "0.5"
tokio  = { version = "1", features = ["full"] }
futures = "0.3"
```

```
export DEEPSEEK_API_KEY=your_key_here
```

---

## Roadmap

- OpenAI-compatible providers
- Structured output support
- `#[tool]` support for custom `serde` types
- More examples

---

## License

MIT OR Apache-2.0