//! Agent streaming state machine.
//!
//! This module is responsible *only* for scheduling and polling — it does not
//! contain any business logic.  All "do actual work" functions live in
//! [`executor`][super::executor]:
//!
//! ```text
//! AgentStream::poll_next
//!   │
//!   ├─ Idle              → spawn run_summarize future
//!   ├─ Summarizing       → poll future → ConnectingStream | FetchingResponse
//!   ├─ FetchingResponse  → poll future → YieldingToolCalls | Done  (yield Token)
//!   ├─ ConnectingStream  → poll future → StreamingChunks
//!   ├─ StreamingChunks   → poll inner stream → yield Token | YieldingToolCalls | Done
//!   ├─ YieldingToolCalls → drain queue → ExecutingTools  (yield ToolCall per item)
//!   ├─ ExecutingTools    → poll future → YieldingToolResults
//!   ├─ YieldingToolResults → drain queue → Idle  (yield ToolResult per item)
//!   └─ Done              → Poll::Ready(None)
//! ```

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::{Stream, StreamExt};

use super::executor::{
    ConnectFuture, ExecFuture, FetchFuture, StreamingData, SummarizeFuture, apply_chunk_delta,
    connect_stream, execute_tools, fetch_response, finalize_stream, raw_to_tool_call_info,
    run_summarize,
};
use crate::agent::agent_core::{AgentEvent, DeepseekAgent, ToolCallResult};
use crate::error::ApiError;

// ── State machine ─────────────────────────────────────────────────────────────

/// Drives an agent through one or more API turns, tool-execution rounds, and
/// summarization passes, emitting [`AgentEvent`]s as a [`Stream`].
///
/// Obtain one by calling [`DeepseekAgent::chat`][crate::agent::DeepseekAgent::chat].
/// Collect it with any `futures::StreamExt` combinator or `while let Some(…)`.
///
/// # Example
///
/// ```no_run
/// use futures::StreamExt;
/// use ds_api::{DeepseekAgent, AgentEvent};
///
/// # #[tokio::main] async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let mut stream = DeepseekAgent::new("sk-...")
///     .with_streaming()
///     .chat("What is 2 + 2?");
///
/// while let Some(event) = stream.next().await {
///     match event? {
///         AgentEvent::Token(text) => print!("{text}"),
///         AgentEvent::ToolCall(info) => println!("\n[calling {}]", info.name),
///         AgentEvent::ToolResult(res) => println!("[result: {}]", res.result),
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub struct AgentStream {
    /// The agent is held here whenever no future has taken ownership of it.
    agent: Option<DeepseekAgent>,
    state: AgentStreamState,
}

/// Every variant is self-contained: it either holds the agent directly or stores
/// a future that will return the agent when it resolves.
pub(crate) enum AgentStreamState {
    /// Waiting to start (or restart after tool results are delivered).
    Idle,
    /// Running `maybe_summarize` before the next API turn.
    Summarizing(SummarizeFuture),
    /// Awaiting a non-streaming API response.
    FetchingResponse(FetchFuture),
    /// Awaiting the initial SSE connection.
    ConnectingStream(ConnectFuture),
    /// Polling an active SSE stream chunk-by-chunk.
    StreamingChunks(Box<StreamingData>),
    /// Yielding individual `ToolCall` preview events before execution starts.
    /// `raw` contains the wire-format calls needed to kick off [`ExecutingTools`].
    YieldingToolCalls {
        pending: VecDeque<crate::agent::agent_core::ToolCallInfo>,
        raw: Vec<crate::raw::request::message::ToolCall>,
    },
    /// Awaiting parallel/sequential tool execution.
    ExecutingTools(ExecFuture),
    /// Yielding individual `ToolResult` events after execution completes.
    YieldingToolResults { pending: VecDeque<ToolCallResult> },
    /// Terminal state — the stream will never produce another item.
    Done,
}

// ── Constructor / accessor ────────────────────────────────────────────────────

impl AgentStream {
    /// Wrap an agent and start in the [`Idle`][AgentStreamState::Idle] state.
    pub fn new(agent: DeepseekAgent) -> Self {
        Self {
            agent: Some(agent),
            state: AgentStreamState::Idle,
        }
    }

    /// Consume the stream and return the agent.
    ///
    /// If the stream finished normally (or was dropped mid-stream), the agent is
    /// returned so callers can continue the conversation without constructing a
    /// new one.
    ///
    /// Returns `None` only if the agent is currently owned by an in-progress
    /// future (i.e. the stream was dropped mid-poll, which is very unusual).
    pub fn into_agent(self) -> Option<DeepseekAgent> {
        match self.state {
            AgentStreamState::StreamingChunks(data) => Some(data.agent),
            _ => self.agent,
        }
    }
}

// ── Stream implementation ─────────────────────────────────────────────────────

impl Stream for AgentStream {
    type Item = Result<AgentEvent, ApiError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            // ── StreamingChunks is handled first to avoid borrow-checker
            //    conflicts: we need to both poll the inner stream *and* replace
            //    `this.state`, which requires owning the data.
            if matches!(this.state, AgentStreamState::StreamingChunks(_)) {
                let mut data = match std::mem::replace(&mut this.state, AgentStreamState::Done) {
                    AgentStreamState::StreamingChunks(d) => d,
                    _ => unreachable!(),
                };

                match data.stream.poll_next_unpin(cx) {
                    Poll::Pending => {
                        this.state = AgentStreamState::StreamingChunks(data);
                        return Poll::Pending;
                    }

                    Poll::Ready(Some(Ok(chunk))) => {
                        let fragment = apply_chunk_delta(&mut data, chunk);
                        this.state = AgentStreamState::StreamingChunks(data);
                        if let Some(text) = fragment {
                            return Poll::Ready(Some(Ok(AgentEvent::Token(text))));
                        }
                        continue;
                    }

                    Poll::Ready(Some(Err(e))) => {
                        // Stream errored — salvage the agent and terminate.
                        this.agent = Some(data.agent);
                        // state stays Done (set above via mem::replace)
                        return Poll::Ready(Some(Err(e)));
                    }

                    Poll::Ready(None) => {
                        // SSE stream ended — assemble full tool calls from buffers.
                        let raw_tool_calls = finalize_stream(&mut data);

                        if raw_tool_calls.is_empty() {
                            this.agent = Some(data.agent);
                            this.state = AgentStreamState::Done;
                            return Poll::Ready(None);
                        }

                        let pending = raw_tool_calls
                            .iter()
                            .map(raw_to_tool_call_info)
                            .collect::<VecDeque<_>>();
                        this.agent = Some(data.agent);
                        this.state = AgentStreamState::YieldingToolCalls {
                            pending,
                            raw: raw_tool_calls,
                        };
                        continue;
                    }
                }
            }

            // ── All other states ──────────────────────────────────────────────
            match &mut this.state {
                AgentStreamState::Done => return Poll::Ready(None),

                AgentStreamState::Idle => {
                    let agent = this.agent.take().expect("agent missing in Idle state");
                    this.state = AgentStreamState::Summarizing(Box::pin(run_summarize(agent)));
                }

                AgentStreamState::Summarizing(fut) => match fut.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(agent) => {
                        this.state = if agent.streaming {
                            AgentStreamState::ConnectingStream(Box::pin(connect_stream(agent)))
                        } else {
                            AgentStreamState::FetchingResponse(Box::pin(fetch_response(agent)))
                        };
                    }
                },

                AgentStreamState::FetchingResponse(fut) => match fut.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready((Err(e), agent)) => {
                        this.agent = Some(agent);
                        this.state = AgentStreamState::Done;
                        return Poll::Ready(Some(Err(e)));
                    }
                    Poll::Ready((Ok(fetch), agent)) => {
                        this.agent = Some(agent);

                        if fetch.raw_tool_calls.is_empty() {
                            this.state = AgentStreamState::Done;
                            return if let Some(text) = fetch.content {
                                Poll::Ready(Some(Ok(AgentEvent::Token(text))))
                            } else {
                                Poll::Ready(None)
                            };
                        }

                        let pending = fetch
                            .raw_tool_calls
                            .iter()
                            .map(raw_to_tool_call_info)
                            .collect::<VecDeque<_>>();

                        // Yield any text content before transitioning.
                        let maybe_text = fetch.content.map(AgentEvent::Token);
                        this.state = AgentStreamState::YieldingToolCalls {
                            pending,
                            raw: fetch.raw_tool_calls,
                        };

                        if let Some(event) = maybe_text {
                            return Poll::Ready(Some(Ok(event)));
                        }
                        continue;
                    }
                },

                AgentStreamState::ConnectingStream(fut) => match fut.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready((Err(e), agent)) => {
                        this.agent = Some(agent);
                        this.state = AgentStreamState::Done;
                        return Poll::Ready(Some(Err(e)));
                    }
                    Poll::Ready((Ok(stream), agent)) => {
                        this.state = AgentStreamState::StreamingChunks(Box::new(StreamingData {
                            stream,
                            agent,
                            content_buf: String::new(),
                            tool_call_bufs: Vec::new(),
                        }));
                        // Loop back to hit the StreamingChunks branch.
                    }
                },

                AgentStreamState::YieldingToolCalls { pending, raw } => {
                    if let Some(info) = pending.pop_front() {
                        return Poll::Ready(Some(Ok(AgentEvent::ToolCall(info))));
                    }
                    // All previews yielded — begin execution.
                    let agent = this
                        .agent
                        .take()
                        .expect("agent missing in YieldingToolCalls");
                    let raw_calls = std::mem::take(raw);
                    this.state =
                        AgentStreamState::ExecutingTools(Box::pin(execute_tools(agent, raw_calls)));
                }

                AgentStreamState::ExecutingTools(fut) => match fut.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready((tools_result, agent)) => {
                        this.agent = Some(agent);
                        this.state = AgentStreamState::YieldingToolResults {
                            pending: tools_result.results.into_iter().collect(),
                        };
                    }
                },

                AgentStreamState::YieldingToolResults { pending } => {
                    if let Some(result) = pending.pop_front() {
                        return Poll::Ready(Some(Ok(AgentEvent::ToolResult(result))));
                    }
                    // All results delivered — loop back for the next API turn.
                    this.state = AgentStreamState::Idle;
                }

                // Handled in the dedicated block above; this arm is unreachable
                // but the compiler cannot verify that without exhaustiveness help.
                AgentStreamState::StreamingChunks(_) => unreachable!(),
            }
        }
    }
}
