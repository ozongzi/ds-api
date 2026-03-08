/*!
Agent module (refactored)

This module splits the previous single-file `agent` implementation into two focused
submodules:

- `agent_core` — the core agent struct, public response/event types and tool
  registration logic.
- `stream` — the asynchronous `AgentStream` state machine that drives API calls
  and tool execution.

We re-export the primary public types here so the crate-level API remains stable:
callers can continue to use `ds_api::DeepseekAgent`, `ds_api::AgentResponse`,
and `ds_api::ToolCallEvent`.
*/

pub mod agent_core;
pub mod stream;

pub use agent_core::{AgentEvent, DeepseekAgent, ToolCallInfo, ToolCallResult};
pub use stream::AgentStream;
