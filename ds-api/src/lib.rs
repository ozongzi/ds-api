pub mod agent;
pub mod api;
pub mod conversation;
pub mod error;
pub mod raw; // raw types remain accessible via `ds_api::raw` but are not the primary public API
pub mod tool;

pub use agent::{AgentResponse, DeepseekAgent, ToolCallEvent};
pub use api::{ApiClient, ApiRequest};
pub use conversation::DeepseekConversation;
pub use tool::Tool;

pub use ds_api_macros::tool as tool_macro;
