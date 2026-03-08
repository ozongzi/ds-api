/*!
ds-api — Rust client for DeepSeek

Quickstart

Example: simple non-streaming request
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

Example: DeepseekAgent with a minimal tool
```no_run
use ds_api::{AgentEvent, DeepseekAgent, tool};
use futures::StreamExt;
use serde_json::json;

struct EchoTool;

#[tool]
impl ds_api::Tool for EchoTool {
    // Example tool method: echo a string back as JSON.
    async fn echo(&self, input: String) -> serde_json::Value {
        json!({ "echo": input })
    }
}

#[tokio::main]
async fn main() {
    // Ensure DEEPSEEK_API_KEY is set in your environment before running this example.
    let token = std::env::var("DEEPSEEK_API_KEY").expect("DEEPSEEK_API_KEY must be set");
    let agent = DeepseekAgent::new(token).add_tool(EchoTool);

    // The agent returns a stream of `AgentEvent` items. Each variant represents
    // a distinct event: assistant text, a tool call request, or a tool result.
    let mut s = agent.chat("Please echo: hello");
    while let Some(event) = s.next().await {
        match event {
            Err(e) => { eprintln!("Error: {e}"); break; }
            Ok(AgentEvent::Token(text)) => println!("Assistant: {}", text),
            Ok(AgentEvent::ToolCall(c)) => println!("Tool call: {}({})", c.name, c.args),
            Ok(AgentEvent::ToolResult(r)) => println!("Result: {} -> {}", r.name, r.result),
        }
    }
}
```

See the crate README for more examples and migration notes.
*/

pub mod agent;
pub mod api;
pub mod conversation;
pub mod error;
pub mod raw; // raw types remain accessible via `ds_api::raw` but are not the primary public API
pub mod tool_trait;

pub use agent::{AgentEvent, DeepseekAgent, ToolCallInfo, ToolCallResult};
pub use api::{ApiClient, ApiRequest};
pub use conversation::{Conversation, LlmSummarizer, SlidingWindowSummarizer};
pub use error::ApiError;
pub use tool_trait::Tool;

pub use ds_api_macros::tool;
