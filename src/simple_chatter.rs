//! 一个简单的DeepSeek聊天客户端，使用Vec<Messages>存储对话历史记录
//! 实现了：
//! - 添加消息
//! - 聊天
//! 没有实现：
//! - 流式响应
//! - 上下文长度过长自动 Summary / 选取部分历史记录

use std::error::Error;

use crate::{normal_chatter::NormalChatter, request::*};

/// 一个简单的DeepSeek聊天客户端，使用Vec<Messages>存储对话历史记录
/// history 中的第一条消息是 System Prompt，后续消息是 User 或 Assistant 消息
pub struct SimpleChatter {
    pub history: Vec<Message>,
    pub chatter: NormalChatter,
}

impl SimpleChatter {
    pub fn new(token: String, system_prompt: String) -> Self {
        Self {
            history: vec![Message::new(Role::System, &system_prompt)],
            chatter: NormalChatter::new(token),
        }
    }

    pub async fn chat<T: AsRef<str>>(&mut self, user_message: T) -> Result<String, Box<dyn Error>> {
        self.chatter.chat(user_message, &mut self.history).await
    }

    pub async fn chat_json<T: AsRef<str>>(
        &mut self,
        user_message: T,
    ) -> Result<serde_json::Value, Box<dyn Error>> {
        self.chatter
            .chat_json(user_message, &mut self.history)
            .await
    }

    pub async fn system_prompt_mut(&mut self) -> &mut String {
        self.history[0].content.as_mut().unwrap()
    }
}
