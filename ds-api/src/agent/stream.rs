use std::pin::Pin;
use std::task::{Context, Poll};

use futures::stream::BoxStream;
use futures::{Stream, StreamExt};
use serde_json::Value;

use crate::agent::agent_core::{AgentResponse, DeepseekAgent, ToolCallEvent};
use crate::conversation::Conversation;
use crate::error::ApiError;
use crate::raw::request::message::{FunctionCall, Message, Role, ToolCall, ToolType};
use crate::raw::ChatCompletionChunk;

// ── Internal result types ────────────────────────────────────────────────────

struct FetchResult {
    content: Option<String>,
    raw_tool_calls: Vec<ToolCall>,
}

struct ToolsResult {
    events: Vec<ToolCallEvent>,
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

type FetchFuture =
    Pin<Box<dyn std::future::Future<Output = (Result<FetchResult, ApiError>, DeepseekAgent)> + Send>>;

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

type ExecFuture =
    Pin<Box<dyn std::future::Future<Output = (ToolsResult, DeepseekAgent)> + Send>>;

// ── State machine ────────────────────────────────────────────────────────────

pub struct AgentStream {
    agent: Option<DeepseekAgent>,
    state: AgentStreamState,
}

enum AgentStreamState {
    Idle,
    FetchingResponse(FetchFuture),
    ConnectingStream(ConnectFuture),
    StreamingChunks(Box<StreamingData>),
    ExecutingTools(ExecFuture),
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
    type Item = Result<AgentResponse, ApiError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            // StreamingChunks is handled first to avoid borrow-checker conflicts when
            // we need to both poll the inner stream and replace `this.state`.
            if matches!(this.state, AgentStreamState::StreamingChunks(_)) {
                let mut data =
                    match std::mem::replace(&mut this.state, AgentStreamState::Done) {
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
                                        if let Some(id) = dtc.id {
                                            if partial.id.is_empty() {
                                                partial.id = id;
                                            }
                                        }
                                        if let Some(func) = dtc.function {
                                            if let Some(args) = func.arguments {
                                                partial.arguments.push_str(&args);
                                            }
                                        }
                                    }
                                }
                            }

                            if let Some(content) = delta.content {
                                if !content.is_empty() {
                                    data.content_buf.push_str(&content);
                                    fragment = Some(content);
                                }
                            }
                        }

                        this.state = AgentStreamState::StreamingChunks(data);

                        if let Some(content) = fragment {
                            return Poll::Ready(Some(Ok(AgentResponse {
                                content: Some(content),
                                tool_calls: vec![],
                            })));
                        }
                        continue;
                    }

                    Poll::Ready(Some(Err(e))) => {
                        // Propagate the stream error; state stays Done.
                        this.agent = Some(data.agent);
                        return Poll::Ready(Some(Err(e)));
                    }

                    Poll::Ready(None) => {
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
                            return Poll::Ready(None);
                        }

                        let preview_events = build_preview(&raw_tool_calls);
                        let fut = Box::pin(execute_tools(data.agent, raw_tool_calls));
                        this.state = AgentStreamState::ExecutingTools(fut);
                        return Poll::Ready(Some(Ok(AgentResponse {
                            content: None,
                            tool_calls: preview_events,
                        })));
                    }
                }
            }

            match &mut this.state {
                AgentStreamState::Done => return Poll::Ready(None),

                AgentStreamState::Idle => {
                    let agent = this.agent.take().expect("agent missing");
                    if agent.streaming {
                        let fut = Box::pin(connect_stream(agent));
                        this.state = AgentStreamState::ConnectingStream(fut);
                    } else {
                        let fut = Box::pin(fetch_response(agent));
                        this.state = AgentStreamState::FetchingResponse(fut);
                    }
                }

                AgentStreamState::FetchingResponse(fut) => {
                    match fut.as_mut().poll(cx) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready((Err(e), agent)) => {
                            this.agent = Some(agent);
                            this.state = AgentStreamState::Done;
                            return Poll::Ready(Some(Err(e)));
                        }
                        Poll::Ready((Ok(fetch), agent)) => {
                            if fetch.raw_tool_calls.is_empty() {
                                this.agent = Some(agent);
                                this.state = AgentStreamState::Done;
                                return Poll::Ready(Some(Ok(AgentResponse {
                                    content: fetch.content,
                                    tool_calls: vec![],
                                })));
                            }

                            let content = fetch.content.clone();
                            let raw_calls = fetch.raw_tool_calls;
                            let preview_events = build_preview(&raw_calls);
                            let fut = Box::pin(execute_tools(agent, raw_calls));
                            this.state = AgentStreamState::ExecutingTools(fut);
                            return Poll::Ready(Some(Ok(AgentResponse {
                                content,
                                tool_calls: preview_events,
                            })));
                        }
                    }
                }

                AgentStreamState::ConnectingStream(fut) => {
                    match fut.as_mut().poll(cx) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready((Err(e), agent)) => {
                            this.agent = Some(agent);
                            this.state = AgentStreamState::Done;
                            return Poll::Ready(Some(Err(e)));
                        }
                        Poll::Ready((Ok(stream), agent)) => {
                            this.state =
                                AgentStreamState::StreamingChunks(Box::new(StreamingData {
                                    stream,
                                    agent,
                                    content_buf: String::new(),
                                    tool_call_bufs: Vec::new(),
                                }));
                        }
                    }
                }

                AgentStreamState::ExecutingTools(fut) => {
                    match fut.as_mut().poll(cx) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready((results, agent)) => {
                            this.agent = Some(agent);
                            this.state = AgentStreamState::Idle;
                            return Poll::Ready(Some(Ok(AgentResponse {
                                content: None,
                                tool_calls: results.events,
                            })));
                        }
                    }
                }

                AgentStreamState::StreamingChunks(_) => unreachable!(),
            }
        }
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn build_preview(raw_calls: &[ToolCall]) -> Vec<ToolCallEvent> {
    raw_calls
        .iter()
        .map(|tc| ToolCallEvent {
            id: tc.id.clone(),
            name: tc.function.name.clone(),
            args: serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null),
            result: Value::Null,
        })
        .collect()
}

fn build_request(agent: &DeepseekAgent) -> crate::api::ApiRequest {
    let history = agent.conversation.history().clone();
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

    let resp = match agent.client.send(req).await {
        Ok(r) => r,
        Err(e) => return (Err(e), agent),
    };

    let choice = match resp.choices.into_iter().next() {
        Some(c) => c,
        None => return (Err(ApiError::Other("empty response: no choices".into())), agent),
    };

    let assistant_msg = choice.message;
    let content = assistant_msg.content.clone();
    let raw_tool_calls = assistant_msg.tool_calls.clone().unwrap_or_default();
    agent.conversation.history_mut().push(assistant_msg);

    (Ok(FetchResult { content, raw_tool_calls }), agent)
}

async fn connect_stream(
    agent: DeepseekAgent,
) -> (
    Result<BoxStream<'static, Result<ChatCompletionChunk, ApiError>>, ApiError>,
    DeepseekAgent,
) {
    let req = build_request(&agent);
    match agent.client.clone().into_stream(req).await {
        Ok(stream) => (Ok(stream), agent),
        Err(e) => (Err(e), agent),
    }
}

async fn execute_tools(
    mut agent: DeepseekAgent,
    raw_tool_calls: Vec<ToolCall>,
) -> (ToolsResult, DeepseekAgent) {
    let mut events = vec![];

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

        events.push(ToolCallEvent {
            id: tc.id,
            name: tc.function.name,
            args,
            result,
        });
    }

    (ToolsResult { events }, agent)
}
