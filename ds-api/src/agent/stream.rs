use std::pin::Pin;
use std::task::{Context, Poll};

use futures::Stream;
use serde_json::Value;

use crate::agent::agent_core::{AgentResponse, DeepseekAgent, ToolCallEvent};
use crate::conversation::Conversation;
use crate::raw::request::message::{Message, Role, ToolCall};

/// API call result (internal)
struct FetchResult {
    content: Option<String>,
    raw_tool_calls: Vec<ToolCall>,
}

// Tools execution result (internal)
struct ToolsResult {
    events: Vec<ToolCallEvent>,
}

/// AgentStream: async driver that advances in phases (fetch -> yield content -> execute tools -> yield tool events)
pub struct AgentStream {
    agent: Option<DeepseekAgent>,
    state: AgentStreamState,
}

enum AgentStreamState {
    Idle,
    // Waiting for API response
    FetchingResponse(
        Pin<Box<dyn std::future::Future<Output = (Option<FetchResult>, DeepseekAgent)> + Send>>,
    ),
    // Content has been yielded; executing tools
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

    pub fn into_agent(self) -> Option<DeepseekAgent> {
        self.agent
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
                                // No tool calls: finish and return content
                                this.agent = Some(agent);
                                this.state = AgentStreamState::Done;
                                return Poll::Ready(Some(AgentResponse {
                                    content: fetch.content,
                                    tool_calls: vec![],
                                }));
                            } else {
                                // There are tool calls:
                                // We want the first yield to return content + tool call requests (preview),
                                // and the second yield to return the tool execution results.
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
                            // Tools finished executing: yield results, then return to Idle for the next round
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

/// Send an API request from the agent and return FetchResult (contains assistant text and potential raw tool calls).
async fn fetch_response(mut agent: DeepseekAgent) -> (Option<FetchResult>, DeepseekAgent) {
    // Build the request using the conversation history
    let history = agent.conversation.history().clone();
    let mut req = crate::api::ApiRequest::builder().messages(history);

    // Attach tools (raw definitions) to the request
    for tool in &agent.tools {
        for raw in tool.raw_tools() {
            req = req.add_tool(raw);
        }
    }

    if !agent.tools.is_empty() {
        req = req.tool_choice_auto();
    }

    // Send the request using the ApiClient owned by the agent
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

    // Add the assistant message into the conversation history
    agent.conversation.history_mut().push(assistant_msg);

    (
        Some(FetchResult {
            content,
            raw_tool_calls,
        }),
        agent,
    )
}

/// Execute tool calls, write tool results back into the conversation history, and return a list of events
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

        // Push the tool's returned result as a tool-role message into the conversation history (to aid subsequent dialog)
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
