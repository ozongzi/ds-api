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
use ds_api::{DeepseekAgent, tool};
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

    // The agent returns a stream of `AgentResponse` events. When the model triggers tool calls,
    // the stream yields a preview (assistant content + tool call requests) followed by the tool results.
    let mut s = agent.chat("Please echo: hello");
    while let Some(ev) = s.next().await {
        if let Some(content) = &ev.content {
            println!("Assistant: {}", content);
        }
        for tc in &ev.tool_calls {
            // ToolCallEvent fields: id, name, args, result
            println!("Tool call: {} -> {}", tc.name, tc.result);
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

// Legacy convenience chat modules `NormalChatter` and `SimpleChatter` were removed
// during the refactor. Use the new `ApiRequest` / `ApiClient` /
// `DeepseekConversation` / `DeepseekAgent` APIs instead.
pub use agent::{AgentResponse, DeepseekAgent, ToolCallEvent};
pub use api::{ApiClient, ApiRequest};
pub use conversation::DeepseekConversation;
pub use tool_trait::Tool;

pub use ds_api_macros::tool;
