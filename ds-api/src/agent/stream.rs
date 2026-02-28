use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use serde_json::Value;

use crate::agent::agent_core::{AgentResponse, DeepseekAgent, ToolCallEvent};
use crate::conversation::Conversation;
use crate::raw::request::message::{Message, Role, ToolCall};

/// API 调用结果（内部）
struct FetchResult {
    content: Option<String>,
    raw_tool_calls: Vec<ToolCall>,
}

// 工具执行结果（内部）
struct ToolsResult {
    events: Vec<ToolCallEvent>,
}

/// AgentStream: 异步驱动器，按阶段（fetch -> yield content -> execute tools -> yield tool events）推进
pub struct AgentStream {
    agent: Option<DeepseekAgent>,
    state: AgentStreamState,
}

enum AgentStreamState {
    Idle,
    // 正在等 API 响应
    FetchingResponse(
        Pin<Box<dyn std::future::Future<Output = (Option<FetchResult>, DeepseekAgent)> + Send>>,
    ),
    // content 已 yield，正在执行 tools
    ExecutingTools(Pin<Box<dyn std::future::Future<Output = (ToolsResult, DeepseekAgent)> + Send>>),
    Done,
}

impl AgentStream {
    pub fn new(agent: DeepseekAgent) -> Self {
        Self {
            agent: Some(agent),
            state: AgentStreamState::Idle,
        }
    }
}

impl Stream for AgentStream {
    type Item = AgentResponse;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();

        loop {
            match &mut this.state {
                AgentStreamState::Done => return Poll::Ready(None),

                AgentStreamState::Idle => {
                    let agent = this.agent.take().expect("agent missing");
                    let fut = Box::pin(fetch_response(agent));
                    this.state = AgentStreamState::FetchingResponse(fut);
                }

                AgentStreamState::FetchingResponse(fut) => {
                    match fut.as_mut().poll(cx) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready((None, agent)) => {
                            this.agent = Some(agent);
                            this.state = AgentStreamState::Done;
                            return Poll::Ready(None);
                        }
                        Poll::Ready((Some(fetch), agent)) => {
                            if fetch.raw_tool_calls.is_empty() {
                                // 没有 tool calls，结束并返回内容
                                this.agent = Some(agent);
                                this.state = AgentStreamState::Done;
                                return Poll::Ready(Some(AgentResponse {
                                    content: fetch.content,
                                    tool_calls: vec![],
                                }));
                            } else {
                                // 有 tool calls：
                                // 我们希望第一次 yield 返回 content + tool call 请求（preview），
                                // 第二次 yield 返回工具的执行结果。
                                let content = fetch.content.clone();

                                // fetch.raw_tool_calls is owned here; take it for preview and clone for execution
                                let raw_calls_owned = fetch.raw_tool_calls;

                                // build preview events: same id/name/args but result = null
                                let preview_events: Vec<ToolCallEvent> = raw_calls_owned
                                    .iter()
                                    .map(|tc| ToolCallEvent {
                                        id: tc.id.clone(),
                                        name: tc.function.name.clone(),
                                        args: serde_json::from_str(&tc.function.arguments)
                                            .unwrap_or(serde_json::Value::Null),
                                        result: serde_json::Value::Null,
                                    })
                                    .collect();

                                // clone raw calls for execution
                                let exec_calls = raw_calls_owned.clone();

                                let fut = Box::pin(execute_tools(agent, exec_calls));
                                this.state = AgentStreamState::ExecutingTools(fut);
                                return Poll::Ready(Some(AgentResponse {
                                    content,
                                    tool_calls: preview_events,
                                }));
                            }
                        }
                    }
                }

                AgentStreamState::ExecutingTools(fut) => {
                    match fut.as_mut().poll(cx) {
                        Poll::Pending => return Poll::Pending,
                        Poll::Ready((results, agent)) => {
                            this.agent = Some(agent);
                            // tool 执行完，yield results，然后回到 Idle 继续下一轮
                            this.state = AgentStreamState::Idle;
                            return Poll::Ready(Some(AgentResponse {
                                content: None,
                                tool_calls: results.events,
                            }));
                        }
                    }
                }
            }
        }
    }
}

/// 从 agent 发起 API 请求并返回 FetchResult（包含 assistant 文本和潜在的 raw tool calls）。
async fn fetch_response(mut agent: DeepseekAgent) -> (Option<FetchResult>, DeepseekAgent) {
    // 使用会话中的历史构建请求
    let history = agent.conversation.history().clone();
    let mut req = crate::api::ApiRequest::builder().messages(history);

    // 将工具（raw definitions）附加到请求
    for tool in &agent.tools {
        for raw in tool.raw_tools() {
            req = req.add_tool(raw);
        }
    }

    if !agent.tools.is_empty() {
        req = req.tool_choice_auto();
    }

    // 使用 agent 自己持有的 ApiClient 发送请求
    let resp = match agent.client.send(req).await {
        Ok(r) => r,
        Err(_) => return (None, agent),
    };

    let choice = match resp.choices.into_iter().next() {
        Some(c) => c,
        None => return (None, agent),
    };

    let assistant_msg = choice.message;
    let content = assistant_msg.content.clone();
    let raw_tool_calls = assistant_msg.tool_calls.clone().unwrap_or_default();

    // assistant 消息加入会话历史
    agent.conversation.history_mut().push(assistant_msg);

    (
        Some(FetchResult {
            content,
            raw_tool_calls,
        }),
        agent,
    )
}

/// 执行工具调用并将工具结果写回会话历史，返回事件列表
async fn execute_tools(
    mut agent: DeepseekAgent,
    raw_tool_calls: Vec<ToolCall>,
) -> (ToolsResult, DeepseekAgent) {
    let mut events = vec![];

    for tc in raw_tool_calls {
        let args: Value = serde_json::from_str(&tc.function.arguments).unwrap_or(Value::Null);

        let result = match agent.tool_index.get(&tc.function.name) {
            Some(&idx) => agent.tools[idx].call(&tc.function.name, args.clone()).await,
            None => serde_json::json!({ "error": format!("unknown tool: {}", tc.function.name) }),
        };

        // 将 tool 返回的结果以工具角色消息加入会话历史（便于后续对话）
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
