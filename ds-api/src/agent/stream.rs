use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures::stream::BoxStream;
use futures::{Stream, StreamExt};
use serde_json::Value;

use crate::agent::agent_core::{AgentEvent, DeepseekAgent, ToolCallInfo, ToolCallResult};
use crate::error::ApiError;
use crate::raw::ChatCompletionChunk;
use crate::raw::request::message::{FunctionCall, Message, Role, ToolCall, ToolType};

type SummarizeFuture = Pin<Box<dyn std::future::Future<Output = DeepseekAgent> + Send>>;

// ── Internal result types ────────────────────────────────────────────────────

struct FetchResult {
    content: Option<String>,
    raw_tool_calls: Vec<ToolCall>,
}

struct ToolsResult {
    results: Vec<ToolCallResult>,
}

// ── Streaming accumulator ────────────────────────────────────────────────────

struct PartialToolCall {
    id: String,
    name: String,
    arguments: String,
}

struct StreamingData {
    stream: BoxStream<'static, Result<ChatCompletionChunk, ApiError>>,
    agent: DeepseekAgent,
    content_buf: String,
    tool_call_bufs: Vec<Option<PartialToolCall>>,
}

// ── Type aliases for future outputs ─────────────────────────────────────────

type FetchFuture = Pin<
    Box<dyn std::future::Future<Output = (Result<FetchResult, ApiError>, DeepseekAgent)> + Send>,
>;

type ConnectFuture = Pin<
    Box<
        dyn std::future::Future<
                Output = (
                    Result<BoxStream<'static, Result<ChatCompletionChunk, ApiError>>, ApiError>,
                    DeepseekAgent,
                ),
            > + Send,
    >,
>;

type ExecFuture = Pin<Box<dyn std::future::Future<Output = (ToolsResult, DeepseekAgent)> + Send>>;

// ── State machine ────────────────────────────────────────────────────────────

pub struct AgentStream {
    /// The agent is held here when no future has ownership of it.
    agent: Option<DeepseekAgent>,
    state: AgentStreamState,
}

enum AgentStreamState {
    Idle,
    /// Running `maybe_summarize` before the next API turn.
    Summarizing(SummarizeFuture),
    FetchingResponse(FetchFuture),
    ConnectingStream(ConnectFuture),
    StreamingChunks(Box<StreamingData>),
    /// Yielding individual `ToolCall` events before kicking off execution.
    /// Carries the queued previews and the raw calls needed to execute them.
    YieldingToolCalls {
        pending: VecDeque<ToolCallInfo>,
        raw: Vec<ToolCall>,
    },
    ExecutingTools(ExecFuture),
    /// Yielding individual `ToolResult` events after execution completes.
    YieldingToolResults {
        pending: VecDeque<ToolCallResult>,
    },
    Done,
}

impl AgentStream {
    pub fn new(agent: DeepseekAgent) -> Self {
        Self {
            agent: Some(agent),
            state: AgentStreamState::Idle,
        }
    }

    pub fn into_agent(self) -> Option<DeepseekAgent> {
        match self.state {
            AgentStreamState::StreamingChunks(data) => Some(data.agent),
            _ => self.agent,
        }
    }
}

impl Stream for AgentStream {
    type Item = Result<AgentEvent, ApiError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            // ── StreamingChunks is extracted first to avoid borrow-checker
            //    conflicts when we need to both poll the inner stream and
            //    replace `this.state`.
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
                        let mut fragment: Option<String> = None;

                        if let Some(choice) = chunk.choices.into_iter().next() {
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
                                fragment = Some(content);
                            }
                        }

                        this.state = AgentStreamState::StreamingChunks(data);

                        if let Some(text) = fragment {
                            return Poll::Ready(Some(Ok(AgentEvent::Token(text))));
                        }
                        continue;
                    }

                    Poll::Ready(Some(Err(e))) => {
                        this.agent = Some(data.agent);
                        // state stays Done
                        return Poll::Ready(Some(Err(e)));
                    }

                    Poll::Ready(None) => {
                        // SSE stream finished — assemble raw tool calls from buffers.
                        let raw_tool_calls: Vec<ToolCall> = data
                            .tool_call_bufs
                            .into_iter()
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
                                Some(data.content_buf)
                            },
                            tool_calls: if raw_tool_calls.is_empty() {
                                None
                            } else {
                                Some(raw_tool_calls.clone())
                            },
                            ..Default::default()
                        };
                        data.agent.conversation.history_mut().push(assistant_msg);

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

            match &mut this.state {
                AgentStreamState::Done => return Poll::Ready(None),

                AgentStreamState::Idle => {
                    let agent = this.agent.take().expect("agent missing");
                    let fut = Box::pin(run_summarize(agent));
                    this.state = AgentStreamState::Summarizing(fut);
                }

                AgentStreamState::Summarizing(fut) => match fut.as_mut().poll(cx) {
                    Poll::Pending => return Poll::Pending,
                    Poll::Ready(agent) => {
                        if agent.streaming {
                            let fut = Box::pin(connect_stream(agent));
                            this.state = AgentStreamState::ConnectingStream(fut);
                        } else {
                            let fut = Box::pin(fetch_response(agent));
                            this.state = AgentStreamState::FetchingResponse(fut);
                        }
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
                        this.state = AgentStreamState::YieldingToolCalls {
                            pending,
                            raw: fetch.raw_tool_calls,
                        };

                        if let Some(text) = fetch.content {
                            return Poll::Ready(Some(Ok(AgentEvent::Token(text))));
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
                    }
                },

                AgentStreamState::YieldingToolCalls { pending, raw } => {
                    if let Some(info) = pending.pop_front() {
                        return Poll::Ready(Some(Ok(AgentEvent::ToolCall(info))));
                    }
                    // All previews yielded — execute the tools.
                    let agent = this.agent.take().expect("agent missing");
                    let raw_calls = std::mem::take(raw);
                    let fut = Box::pin(execute_tools(agent, raw_calls));
                    this.state = AgentStreamState::ExecutingTools(fut);
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
                    // All results yielded — loop back for another API turn.
                    this.state = AgentStreamState::Idle;
                }

                AgentStreamState::StreamingChunks(_) => unreachable!(),
            }
        }
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn raw_to_tool_call_info(tc: &ToolCall) -> ToolCallInfo {
    ToolCallInfo {
        id: tc.id.clone(),
        name: tc.function.name.clone(),
        args: serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null),
    }
}

/// Run `maybe_summarize` on the agent's conversation and return the agent.
async fn run_summarize(mut agent: DeepseekAgent) -> DeepseekAgent {
    agent.conversation.maybe_summarize().await;
    agent
}

fn build_request(agent: &DeepseekAgent) -> crate::api::ApiRequest {
    let history = agent.conversation.history().to_vec();
    let mut req = crate::api::ApiRequest::builder().messages(history);
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

async fn fetch_response(
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

async fn connect_stream(
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

async fn execute_tools(
    mut agent: DeepseekAgent,
    raw_tool_calls: Vec<ToolCall>,
) -> (ToolsResult, DeepseekAgent) {
    let mut results = vec![];

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
