//! Agent executor — pure business logic for driving one API turn.
//!
//! This module owns all the "do actual work" functions that were previously
//! scattered at the bottom of `stream.rs`:
//!
//! | Function | Responsibility |
//! |---|---|
//! | [`build_request`] | Assemble an [`ApiRequest`] from current history + tools. |
//! | [`run_summarize`] | Invoke `maybe_summarize` and hand the agent back. |
//! | [`fetch_response`] | Non-streaming API call; returns content + raw tool calls. |
//! | [`connect_stream`] | Open an SSE stream and hand back the `BoxStream`. |
//! | [`execute_tools`] | Dispatch all pending tool calls and collect results. |
//! | [`raw_to_tool_call_info`] | Convert a raw [`ToolCall`] wire object to [`ToolCallInfo`]. |
//!
//! The streaming state machine in [`stream`][super::stream] is the only consumer of
//! this module; nothing in here knows about [`Poll`] or [`Context`].  That separation
//! makes it straightforward to add retry logic, timeouts, or parallel tool execution
//! in the future without touching the state machine.

use futures::stream::BoxStream;
use serde_json::Value;

use crate::agent::agent_core::{DeepseekAgent, ToolCallInfo, ToolCallResult};
use crate::api::ApiRequest;
use crate::error::ApiError;
use crate::raw::ChatCompletionChunk;
use crate::raw::request::message::{FunctionCall, Message, Role, ToolCall, ToolType};

// ── Internal result types ─────────────────────────────────────────────────────

/// Outcome of a completed non-streaming API fetch.
pub(crate) struct FetchResult {
    /// The assistant's text content, if any.
    pub(crate) content: Option<String>,
    /// Raw tool-call objects requested by the model.
    pub(crate) raw_tool_calls: Vec<ToolCall>,
}

/// Outcome of a completed tool-execution pass.
pub(crate) struct ToolsResult {
    /// One [`ToolCallResult`] per dispatched tool call, in call order.
    pub(crate) results: Vec<ToolCallResult>,
}

// ── Streaming accumulator ─────────────────────────────────────────────────────

/// Accumulates a single tool-call's incremental SSE deltas until the stream ends.
pub(crate) struct PartialToolCall {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) arguments: String,
}

/// All mutable state needed while consuming an SSE stream.
///
/// Boxed by the caller so it fits neatly in one state-machine variant without
/// blowing up the size of every other variant.
pub(crate) struct StreamingData {
    pub(crate) stream: BoxStream<'static, Result<ChatCompletionChunk, ApiError>>,
    pub(crate) agent: DeepseekAgent,
    /// Accumulated text content across all deltas for the current turn.
    pub(crate) content_buf: String,
    /// Per-index partial tool-call buffers; sparse — may contain `None` gaps.
    pub(crate) tool_call_bufs: Vec<Option<PartialToolCall>>,
}

// ── Type aliases for futures returned by this module ─────────────────────────

/// Future produced by [`fetch_response`].
pub(crate) type FetchFuture = std::pin::Pin<
    Box<dyn std::future::Future<Output = (Result<FetchResult, ApiError>, DeepseekAgent)> + Send>,
>;

/// Future produced by [`connect_stream`].
pub(crate) type ConnectFuture = std::pin::Pin<
    Box<
        dyn std::future::Future<
                Output = (
                    Result<BoxStream<'static, Result<ChatCompletionChunk, ApiError>>, ApiError>,
                    DeepseekAgent,
                ),
            > + Send,
    >,
>;

/// Future produced by [`execute_tools`].
pub(crate) type ExecFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = (ToolsResult, DeepseekAgent)> + Send>>;

/// Future produced by [`run_summarize`].
pub(crate) type SummarizeFuture =
    std::pin::Pin<Box<dyn std::future::Future<Output = DeepseekAgent> + Send>>;

// ── Public helpers ────────────────────────────────────────────────────────────

/// Convert a raw wire-format [`ToolCall`] into the public [`ToolCallInfo`] type.
///
/// Arguments are parsed from their JSON string representation; malformed JSON
/// falls back to `Value::Null` rather than propagating an error.
pub(crate) fn raw_to_tool_call_info(tc: &ToolCall) -> ToolCallInfo {
    ToolCallInfo {
        id: tc.id.clone(),
        name: tc.function.name.clone(),
        args: serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null),
    }
}

// ── Business-logic functions ──────────────────────────────────────────────────

/// Assemble an [`ApiRequest`] from the agent's current conversation history and
/// registered tools.
///
/// If the agent has at least one tool registered, `tool_choice` is set to `auto`
/// so the model can freely decide whether to call a tool.
pub(crate) fn build_request(agent: &DeepseekAgent) -> ApiRequest {
    let history = agent.conversation.history().to_vec();
    let mut req = ApiRequest::builder().messages(history);
    for tool in &agent.tools {
        for raw in tool.raw_tools() {
            req = req.add_tool(raw);
        }
    }
    if !agent.tools.is_empty() {
        req = req.tool_choice_auto();
    }
    req
}

/// Run `maybe_summarize` on the agent's conversation and return the agent.
///
/// Ownership of the agent is taken so the future can be stored in the state
/// machine without lifetime complications.
pub(crate) async fn run_summarize(mut agent: DeepseekAgent) -> DeepseekAgent {
    agent.conversation.maybe_summarize().await;
    agent
}

/// Perform a single non-streaming API turn.
///
/// On success, the assistant's reply message is appended to the conversation
/// history before returning.  On failure, the agent is returned alongside the
/// error so the state machine can store it safely.
///
/// Returns `(Result<FetchResult, ApiError>, DeepseekAgent)` so ownership is
/// always transferred back to the caller regardless of outcome.
pub(crate) async fn fetch_response(
    mut agent: DeepseekAgent,
) -> (Result<FetchResult, ApiError>, DeepseekAgent) {
    let req = build_request(&agent);

    let resp = match agent.conversation.client.send(req).await {
        Ok(r) => r,
        Err(e) => return (Err(e), agent),
    };

    let choice = match resp.choices.into_iter().next() {
        Some(c) => c,
        None => {
            return (
                Err(ApiError::Other("empty response: no choices".into())),
                agent,
            );
        }
    };

    let assistant_msg = choice.message;
    let content = assistant_msg.content.clone();
    let raw_tool_calls = assistant_msg.tool_calls.clone().unwrap_or_default();
    agent.conversation.history_mut().push(assistant_msg);

    (
        Ok(FetchResult {
            content,
            raw_tool_calls,
        }),
        agent,
    )
}

/// Open an SSE stream for the current turn.
///
/// The [`ApiRequest`] is built from the agent's current state.  The agent is
/// returned alongside the stream so the state machine can transition into
/// [`StreamingChunks`][super::stream::AgentStreamState::StreamingChunks].
///
/// Returns `(Result<BoxStream<…>, ApiError>, DeepseekAgent)` for the same
/// ownership-transfer reason as [`fetch_response`].
pub(crate) async fn connect_stream(
    agent: DeepseekAgent,
) -> (
    Result<BoxStream<'static, Result<ChatCompletionChunk, ApiError>>, ApiError>,
    DeepseekAgent,
) {
    let req = build_request(&agent);
    match agent.conversation.client.clone().into_stream(req).await {
        Ok(stream) => (Ok(stream), agent),
        Err(e) => (Err(e), agent),
    }
}

/// Execute all pending tool calls sequentially and collect results.
///
/// For each [`ToolCall`]:
/// 1. The corresponding tool implementation is looked up by name.
/// 2. The tool is called with the parsed argument [`Value`].
/// 3. A `Role::Tool` message is appended to the conversation history so the
///    model can see the result on the next turn.
/// 4. A [`ToolCallResult`] is pushed to the results list.
///
/// Unknown tool names produce an error-shaped JSON result rather than panicking,
/// so a misconfigured agent degrades gracefully.
///
/// Returns `(ToolsResult, DeepseekAgent)` — the agent is returned so the state
/// machine can reclaim ownership after the future resolves.
pub(crate) async fn execute_tools(
    mut agent: DeepseekAgent,
    raw_tool_calls: Vec<ToolCall>,
) -> (ToolsResult, DeepseekAgent) {
    let mut results = Vec::with_capacity(raw_tool_calls.len());

    for tc in raw_tool_calls {
        let args: Value = serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);

        let result = match agent.tool_index.get(&tc.function.name) {
            Some(&idx) => agent.tools[idx].call(&tc.function.name, args.clone()).await,
            None => {
                serde_json::json!({ "error": format!("unknown tool: {}", tc.function.name) })
            }
        };

        agent.conversation.history_mut().push(Message {
            role: Role::Tool,
            content: Some(result.to_string()),
            tool_call_id: Some(tc.id.clone()),
            ..Default::default()
        });

        results.push(ToolCallResult {
            id: tc.id,
            name: tc.function.name,
            args,
            result,
        });
    }

    (ToolsResult { results }, agent)
}

/// Finalize a completed SSE stream by assembling full [`ToolCall`] objects from
/// the per-index [`PartialToolCall`] buffers and recording the assistant turn in
/// history.
///
/// Returns the assembled raw tool calls (empty vec if the turn had no tool use).
pub(crate) fn finalize_stream(data: &mut StreamingData) -> Vec<ToolCall> {
    let raw_tool_calls: Vec<ToolCall> = data
        .tool_call_bufs
        .drain(..)
        .flatten()
        .map(|p| ToolCall {
            id: p.id,
            r#type: ToolType::Function,
            function: FunctionCall {
                name: p.name,
                arguments: p.arguments,
            },
        })
        .collect();

    let assistant_msg = Message {
        role: Role::Assistant,
        content: if data.content_buf.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut data.content_buf))
        },
        tool_calls: if raw_tool_calls.is_empty() {
            None
        } else {
            Some(raw_tool_calls.clone())
        },
        ..Default::default()
    };
    data.agent.conversation.history_mut().push(assistant_msg);

    raw_tool_calls
}

/// Apply a single SSE chunk delta to the [`StreamingData`] accumulator.
///
/// Returns any text fragment that should be yielded as an [`AgentEvent::Token`],
/// or `None` if the chunk carried only tool-call delta or was empty.
pub(crate) fn apply_chunk_delta(
    data: &mut StreamingData,
    chunk: crate::raw::ChatCompletionChunk,
) -> Option<String> {
    let choice = chunk.choices.into_iter().next()?;
    let delta = choice.delta;

    if let Some(dtcs) = delta.tool_calls {
        for dtc in dtcs {
            let idx = dtc.index as usize;
            if data.tool_call_bufs.len() <= idx {
                data.tool_call_bufs.resize_with(idx + 1, || None);
            }
            let entry = &mut data.tool_call_bufs[idx];
            if entry.is_none() {
                *entry = Some(PartialToolCall {
                    id: dtc.id.clone().unwrap_or_default(),
                    name: dtc
                        .function
                        .as_ref()
                        .and_then(|f| f.name.clone())
                        .unwrap_or_default(),
                    arguments: String::new(),
                });
            }
            if let Some(partial) = entry.as_mut() {
                if let Some(id) = dtc.id
                    && partial.id.is_empty()
                {
                    partial.id = id;
                }
                if let Some(func) = dtc.function
                    && let Some(args) = func.arguments
                {
                    partial.arguments.push_str(&args);
                }
            }
        }
    }

    if let Some(content) = delta.content
        && !content.is_empty()
    {
        data.content_buf.push_str(&content);
        return Some(content);
    }

    None
}
