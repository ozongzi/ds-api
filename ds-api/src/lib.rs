/*!
ds-api — Rust client for DeepSeek

# Quickstart

## Simple non-streaming request
```no_run
use ds_api::{ApiClient, ApiRequest};
use ds_api::raw::request::message::Message;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Set DEEPSEEK_API_KEY in your environment before running this example.
    let token = std::env::var("DEEPSEEK_API_KEY")?;
    let client = ApiClient::new(token);

    let req = ApiRequest::deepseek_chat(vec![
        Message::new(ds_api::raw::request::message::Role::User, "Hello from Rust"),
    ])
    .max_tokens(150)
    .json();

    let resp = client.send(req).await?;
    // Print debug representation of the response; adapt to your needs.
    println!("Response: {:?}", resp);
    Ok(())
}
```

## Agent with a tool
```no_run
use ds_api::{AgentEvent, DeepseekAgent, tool};
use futures::StreamExt;
use serde_json::{Value, json};

struct EchoTool;

#[tool]
impl ds_api::Tool for EchoTool {
    /// Echo the input back.
    /// input: the string to echo
    async fn echo(&self, input: String) -> Value {
        json!({ "echo": input })
    }
}

#[tokio::main]
async fn main() {
    let token = std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY must be set");
    let mut stream = DeepseekAgent::new(token).add_tool(EchoTool).chat("Please echo: hello");
    while let Some(event) = stream.next().await {
        match event {
            Err(e) => { eprintln!("Error: {e}"); break; }
            Ok(AgentEvent::Token(text))   => println!("Assistant: {text}"),
            Ok(AgentEvent::ToolCall(c))   => println!("calling {}({})", c.name, c.delta),
            Ok(AgentEvent::ToolResult(r)) => println!("result: {} -> {}", r.name, r.result),
            Ok(_) => {}
        }
    }
}
```

## Injecting messages mid-run

[`DeepseekAgent::with_interrupt_channel`] returns a sender you can use to push
messages into the agent while it's still running. Messages are picked up after
the current tool-execution round and added to the conversation before the next
API turn.

```no_run
use ds_api::{AgentEvent, DeepseekAgent, tool};
use futures::StreamExt;
use serde_json::{Value, json};
use tokio::time::{Duration, sleep};

struct SlowTool;

#[tool]
impl ds_api::Tool for SlowTool {
    /// Do some slow work and return a result.
    async fn slow_work(&self) -> Value {
        sleep(Duration::from_secs(1)).await;
        json!({ "status": "done" })
    }
}

#[tokio::main]
async fn main() {
    let token = std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY must be set");

    // Build the agent and obtain the sender half of the interrupt channel.
    let (agent, tx) = DeepseekAgent::new(token)
        .with_streaming()
        .add_tool(SlowTool)
        .with_interrupt_channel();

    // Inject a follow-up message from another task while tools are running.
    tokio::spawn(async move {
        sleep(Duration::from_millis(500)).await;
        tx.send("Actually, please summarise the result in one sentence.".into()).unwrap();
    });

    let mut stream = agent.chat("Please run slow_work.");
    while let Some(event) = stream.next().await {
        match event {
            Err(e) => { eprintln!("Error: {e}"); break; }
            Ok(AgentEvent::Token(text))   => print!("{text}"),
            Ok(AgentEvent::ToolCall(c))   => { if c.delta.is_empty() { println!("\n[calling {}]", c.name) } }
            Ok(AgentEvent::ToolResult(r)) => println!("\n[result] {}", r.result),
            Ok(_) => {}
        }
    }
}
```

See the crate README and the `interrupt` example for more details.

## Inspecting conversation history

After recovering the agent from a finished stream you can read the full
conversation history with [`DeepseekAgent::history`]:

```no_run
use ds_api::DeepseekAgent;
use futures::StreamExt;

# #[tokio::main] async fn main() {
let token = std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY must be set");
let mut stream = DeepseekAgent::new(token).chat("Hello!");
while let Some(_) = stream.next().await {}

if let Some(agent) = stream.into_agent() {
    for msg in agent.history() {
        println!("{:?}: {:?}", msg.role, msg.content);
    }
}
# }
```

See the crate README for more examples and migration notes.
*/

pub mod agent;
pub mod api;
pub mod conversation;
pub mod error;
#[cfg(feature = "mcp")]
pub mod mcp;
pub mod raw; // raw types remain accessible via `ds_api::raw` but are not the primary public API
pub mod tool_trait;

pub use agent::{AgentEvent, DeepseekAgent, ToolCallChunk, ToolCallResult};
pub use api::{ApiClient, ApiRequest};
pub use conversation::{Conversation, LlmSummarizer, SlidingWindowSummarizer};
pub use error::ApiError;

pub use tool_trait::Tool;
pub use tool_trait::ToolBundle;

pub use ds_api_macros::tool;

#[cfg(feature = "mcp")]
pub use mcp::McpTool;
