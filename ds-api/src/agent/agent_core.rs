use std::collections::HashMap;

use crate::api::ApiClient;
use crate::conversation::{Conversation, DeepseekConversation};
use crate::raw::request::message::{Message, Role};
use crate::tool_trait::Tool;
use serde_json::Value;

/// 工具调用事件（结果）
#[derive(Debug, Clone)]
pub struct ToolCallEvent {
    pub id: String,
    pub name: String,
    pub args: Value,
    pub result: Value,
}

/// Agent 对外的单次响应：可能包含 assistant 的文本内容或工具调用事件
#[derive(Debug, Clone)]
pub struct AgentResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCallEvent>,
}

/// DeepseekAgent：封装一个会话（Conversation）和工具集合，负责协调 API 调用与工具执行。
///
/// 字段使用 `pub(crate)` 可见性，以便同一模块下的 `stream` 子模块能够访问内部状态（如 `tools`、`tool_index` 等）。
pub struct DeepseekAgent {
    pub(crate) client: ApiClient,
    pub(crate) conversation: DeepseekConversation,
    pub(crate) tools: Vec<Box<dyn Tool>>,
    pub(crate) tool_index: HashMap<String, usize>,
}

impl DeepseekAgent {
    /// 使用 token 创建 Agent（内部创建 ApiClient 并传入 Conversation）
    pub fn new(token: impl Into<String>) -> Self {
        let client = ApiClient::new(token.into());
        let conversation = DeepseekConversation::new(client.clone());
        Self {
            client,
            conversation,
            tools: vec![],
            tool_index: HashMap::new(),
        }
    }

    /// 添加工具（支持链式调用）
    pub fn add_tool<TT: Tool + 'static>(mut self, tool: TT) -> Self {
        let idx = self.tools.len();
        for raw in tool.raw_tools() {
            // raw.function.name 为工具在协议层的名称，用于匹配工具调用请求
            self.tool_index.insert(raw.function.name.clone(), idx);
        }
        self.tools.push(Box::new(tool));
        self
    }

    /// 向会话添加用户消息并返回一个 stream（AgentStream）来驱动对话请求与工具执行
    ///
    /// 这里返回类型写为完全路径，依赖于同模块下的 `stream` 子模块提供 `AgentStream`。
    pub fn chat(mut self, user_message: &str) -> crate::agent::stream::AgentStream {
        self.conversation.push_user_input(user_message.to_string());
        crate::agent::stream::AgentStream::new(self)
    }

    /// 设置自定义 system prompt（以便在会话开始时注入系统提示）
    /// 这是链式 builder 风格，返回 self 以便继续链式调用。
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        let p = prompt.into();
        // 添加一条 system message 到 conversation history
        self.conversation
            .add_message(Message::new(Role::System, p.as_str()));
        self
    }
}
