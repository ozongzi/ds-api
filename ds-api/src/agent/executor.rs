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
//!
//! The streaming state machine in [`stream`][super::stream] is the only consumer of
//! this module; nothing in here knows about [`Poll`] or [`Context`].  That separation
//! makes it straightforward to add retry logic, timeouts, or parallel tool execution
//! in the future without touching the state machine.

use futures::stream::BoxStream;
use serde_json::Value;

use crate::agent::agent_core::{DeepseekAgent, ToolCallResult};
use crate::api::ApiRequest;
use crate::error::ApiError;
use crate::raw::ChatCompletionChunk;
use crate::raw::request::message::{FunctionCall, Message, Role, ToolCall, ToolType};

// ── Internal result types ─────────────────────────────────────────────────────

/// Outcome of a completed non-streaming API fetch.
pub(crate) struct FetchResult {
    /// The assistant's text content, if any.
    pub(crate) content: Option<String>,
    /// Reasoning/thinking content produced by the model, if any.
    pub(crate) reasoning_content: Option<String>,
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
    /// Accumulated reasoning/thinking content across all deltas for the current turn.
    pub(crate) reasoning_buf: String,
    /// Per-index partial tool-call buffers; sparse — may contain `None` gaps.
    pub(crate) tool_call_bufs: Vec<Option<PartialToolCall>>,
}

/// An incremental streaming event produced by [`apply_chunk_delta`].
pub(crate) enum ChunkEvent {
    /// A text fragment from the assistant.
    Token(String),
    /// A reasoning/thinking fragment (deepseek-reasoner).
    ReasoningToken(String),
    /// A tool call chunk: `(id, name, delta)`.  First emission has empty `delta`
    /// and is the signal that a new tool call has started; subsequent emissions
    /// carry incremental argument JSON fragments.
    ToolCallChunk {
        id: String,
        name: String,
        delta: String,
    },
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

// ── Business-logic functions ──────────────────────────────────────────────────

/// Assemble an [`ApiRequest`] from the agent's current conversation history and
/// registered tools.
///
/// If the agent has at least one tool registered, `tool_choice` is set to `auto`
/// so the model can freely decide whether to call a tool.
pub(crate) fn build_request(agent: &DeepseekAgent) -> ApiRequest {
    let history = agent.conversation.history().to_vec();
    let mut req = ApiRequest::builder()
        .with_model(agent.model.clone())
        .messages(history);
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
    let reasoning_content = assistant_msg.reasoning_content.clone();
    let raw_tool_calls = assistant_msg.tool_calls.clone().unwrap_or_default();
    // Keep reasoning_content in history so it can be sent back within the same
    // Turn (required by deepseek-reasoner when tool calls are involved).
    // It will be stripped at the start of the next Turn in drain_interrupts.
    agent.conversation.history_mut().push(assistant_msg);

    (
        Ok(FetchResult {
            content,
            reasoning_content,
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

        // Prepare a result value and an abort flag.
        let mut result = serde_json::json!({ "error": "unknown tool" });
        let mut aborted = false;

        if let Some(&idx) = agent.tool_index.get(&tc.function.name) {
            // If we have an interrupt receiver, race the tool call against it.
            if let Some(rx) = agent.interrupt_rx.as_mut() {
                tokio::select! {
                    res = agent.tools[idx].call(&tc.function.name, args.clone()) => {
                        result = res;
                    }
                    maybe_msg = rx.recv() => {
                        // An interrupt arrived while the tool was running.
                        if let Some(msg) = maybe_msg {
                            agent.conversation.history_mut().push(Message::user(&msg));
                            // Drain any remaining queued interrupts to preserve order.
                            while let Ok(more) = rx.try_recv() {
                                agent.conversation.history_mut().push(Message::user(&more));
                            }
                        }
                        // Mark the tool as aborted and stop executing further tools.
                        result = serde_json::json!({ "error": "aborted by interrupt" });
                        aborted = true;
                    }
                }
            } else {
                // No interrupt channel — await tool normally.
                result = agent.tools[idx].call(&tc.function.name, args.clone()).await;
            }
        } else {
            result = serde_json::json!({ "error": format!("unknown tool: {}", tc.function.name) });
        }

        agent.conversation.history_mut().push(Message {
            role: Role::Tool,
            content: Some(result.to_string()),
            tool_call_id: Some(tc.id.clone()),
            ..Default::default()
        });

        results.push(ToolCallResult {
            id: tc.id,
            name: tc.function.name,
            args: tc.function.arguments,
            result,
        });

        if aborted {
            break;
        }
    }

    // Drain any user messages that arrived while tools were executing and
    // append them to the history so the model sees them on the next turn.
    if let Some(rx) = agent.interrupt_rx.as_mut() {
        while let Ok(msg) = rx.try_recv() {
            agent.conversation.history_mut().push(Message::user(&msg));
        }
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
        // Keep reasoning_content in history so it can be sent back within the
        // same Turn (required by deepseek-reasoner when tool calls are involved).
        // It will be stripped at the start of the next Turn in drain_interrupts.
        reasoning_content: if data.reasoning_buf.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut data.reasoning_buf))
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
/// Returns a list of zero or more [`ChunkEvent`]s to be yielded to the caller.
/// In practice at most one event is returned per chunk, but the vec keeps the
/// API uniform across all cases.
pub(crate) fn apply_chunk_delta(
    data: &mut StreamingData,
    chunk: crate::raw::ChatCompletionChunk,
) -> Vec<ChunkEvent> {
    let choice = match chunk.choices.into_iter().next() {
        Some(c) => c,
        None => return vec![],
    };
    let delta = choice.delta;

    if let Some(dtcs) = delta.tool_calls {
        let mut events = Vec::new();
        for dtc in dtcs {
            let idx = dtc.index as usize;
            if data.tool_call_bufs.len() <= idx {
                data.tool_call_bufs.resize_with(idx + 1, || None);
            }
            let entry = &mut data.tool_call_bufs[idx];
            if entry.is_none() {
                // First chunk for this tool call — name and id arrive here.
                let id = dtc.id.clone().unwrap_or_default();
                let name = dtc
                    .function
                    .as_ref()
                    .and_then(|f| f.name.clone())
                    .unwrap_or_default();
                events.push(ChunkEvent::ToolCallChunk {
                    id: id.clone(),
                    name: name.clone(),
                    delta: String::new(),
                });
                *entry = Some(PartialToolCall {
                    id,
                    name,
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
                    && !args.is_empty()
                {
                    partial.arguments.push_str(&args);
                    events.push(ChunkEvent::ToolCallChunk {
                        id: partial.id.clone(),
                        name: partial.name.clone(),
                        delta: args,
                    });
                }
            }
        }
        return events;
    }

    if let Some(reasoning) = delta.reasoning_content
        && !reasoning.is_empty()
    {
        data.reasoning_buf.push_str(&reasoning);
        return vec![ChunkEvent::ReasoningToken(reasoning)];
    }

    if let Some(content) = delta.content
        && !content.is_empty()
    {
        data.content_buf.push_str(&content);
        return vec![ChunkEvent::Token(content)];
    }

    vec![]
}
