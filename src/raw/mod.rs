//! Basic usage example for the DeepSeek API Raw Structures
//!
//! This example demonstrates how to use the refactored module structure.
//!
//! ```rust
//! use ds_api::raw::{ChatCompletionRequest, Message, Model, Role, Tool, ToolChoice, ToolChoiceType};
//! use serde_json::json;
//!
//! fn main() {
//!     // Example 1: Basic chat completion request
//!     let basic_request = ChatCompletionRequest {
//!         messages: vec![
//!             Message {
//!                 role: Role::System,
//!                 content: Some("You are a helpful assistant.".to_string()),
//!                 name: None,
//!                 tool_call_id: None,
//!                 tool_calls: None,
//!                 reasoning_content: None,
//!                 prefix: None,
//!             },
//!             Message {
//!                 role: Role::User,
//!                 content: Some("What is the capital of France?".to_string()),
//!                 name: None,
//!                 tool_call_id: None,
//!                 tool_calls: None,
//!                 reasoning_content: None,
//!                 prefix: None,
//!             },
//!         ],
//!         model: Model::DeepseekChat,
//!         thinking: None,
//!         frequency_penalty: None,
//!         max_tokens: Some(100),
//!         presence_penalty: None,
//!         response_format: None,
//!         stop: None,
//!         stream: Some(false),
//!         stream_options: None,
//!         temperature: Some(0.7),
//!         top_p: None,
//!         tools: None,
//!         tool_choice: None,
//!         logprobs: None,
//!         top_logprobs: None,
//!     };
//!
//!     println!("Basic request JSON:");
//!     let json = serde_json::to_string_pretty(&basic_request).unwrap();
//!     println!("{}\n", json);
//!
//!     // Example 2: Request with tools
//!     let tool_request = ChatCompletionRequest {
//!         messages: vec![Message {
//!             role: Role::User,
//!             content: Some("What's the weather like in Tokyo?".to_string()),
//!             name: None,
//!             tool_call_id: None,
//!             tool_calls: None,
//!             reasoning_content: None,
//!             prefix: None,
//!         }],
//!         model: Model::DeepseekChat,
//!         thinking: None,
//!         frequency_penalty: None,
//!         max_tokens: Some(200),
//!         presence_penalty: None,
//!         response_format: None,
//!         stop: None,
//!         stream: Some(false),
//!         stream_options: None,
//!         temperature: Some(0.8),
//!         top_p: None,
//!         tools: Some(vec![Tool {
//!             r#type: ds_api::raw::ToolType::Function,
//!             function: ds_api::raw::Function {
//!                 name: "get_weather".to_string(),
//!                 description: Some("Get the current weather for a location".to_string()),
//!                 parameters: json!({
//!                     "type": "object",
//!                     "properties": {
//!                         "location": {
//!                             "type": "string",
//!                             "description": "The city and country, e.g. Tokyo, Japan"
//!                         }
//!                     },
//!                     "required": ["location"]
//!                 }),
//!                 strict: Some(true),
//!             },
//!         }]),
//!         tool_choice: Some(ToolChoice::String(ToolChoiceType::Auto)),
//!         logprobs: None,
//!         top_logprobs: None,
//!     };
//!
//!     println!("Tool request JSON:");
//!     let json = serde_json::to_string_pretty(&tool_request).unwrap();
//!     println!("{}\n", json);
//!
//!     // Example 3: Using the DeepSeek Reasoner model
//!     let reasoner_request = ChatCompletionRequest {
//!         messages: vec![
//!             Message {
//!                 role: Role::System,
//!                 content: Some(
//!                     "You are a helpful assistant that explains your reasoning.".to_string(),
//!                 ),
//!                 name: None,
//!                 tool_call_id: None,
//!                 tool_calls: None,
//!                 reasoning_content: None,
//!                 prefix: None,
//!             },
//!             Message {
//!                 role: Role::User,
//!                 content: Some("Solve this math problem: What is 15% of 200?".to_string()),
//!                 name: None,
//!                 tool_call_id: None,
//!                 tool_calls: None,
//!                 reasoning_content: None,
//!                 prefix: None,
//!             },
//!         ],
//!         model: Model::DeepseekReasoner,
//!         thinking: Some(ds_api::raw::Thinking {
//!             r#type: ds_api::raw::ThinkingType::Enabled,
//!         }),
//!         frequency_penalty: None,
//!         max_tokens: Some(150),
//!         presence_penalty: None,
//!         response_format: None,
//!         stop: None,
//!         stream: Some(false),
//!         stream_options: None,
//!         temperature: Some(0.3),
//!         top_p: None,
//!         tools: None,
//!         tool_choice: None,
//!         logprobs: None,
//!         top_logprobs: None,
//!     };
//!
//!     println!("Reasoner request JSON:");
//!     let json = serde_json::to_string_pretty(&reasoner_request).unwrap();
//!     println!("{}\n", json);
//!
//!     // Example 4: JSON mode response format
//!     let json_request = ChatCompletionRequest {
//!         messages: vec![
//!             Message {
//!                 role: Role::System,
//!                 content: Some("You are a helpful assistant that always responds in valid JSON format.".to_string()),
//!                 name: None,
//!                 tool_call_id: None,
//!                 tool_calls: None,
//!                 reasoning_content: None,
//!                 prefix: None,
//!             },
//!             Message {
//!                 role: Role::User,
//!                 content: Some("Give me information about Paris in JSON format with fields: name, country, population, and landmarks.".to_string()),
//!                 name: None,
//!                 tool_call_id: None,
//!                 tool_calls: None,
//!                 reasoning_content: None,
//!                 prefix: None,
//!             },
//!         ],
//!         model: Model::DeepseekChat,
//!         thinking: None,
//!         frequency_penalty: None,
//!         max_tokens: Some(200),
//!         presence_penalty: None,
//!         response_format: Some(ds_api::raw::ResponseFormat {
//!             r#type: ds_api::raw::ResponseFormatType::JsonObject,
//!         }),
//!         stop: None,
//!         stream: Some(false),
//!         stream_options: None,
//!         temperature: Some(0.5),
//!         top_p: None,
//!         tools: None,
//!         tool_choice: None,
//!         logprobs: None,
//!         top_logprobs: None,
//!     };
//!
//!     println!("JSON mode request:");
//!     let json = serde_json::to_string_pretty(&json_request).unwrap();
//!     println!("{}\n", json);
//! }
//! ```
pub mod request;
pub mod response;

pub use request::*;
pub use response::*;
