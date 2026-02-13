//! 一个简单的DeepSeek聊天客户端，使用自定义的函数存储对话历史记录
//! 实现了：
//! - 添加消息
//! - 聊天
//! 没有实现：
//! - 流式响应
//! - 上下文长度过长自动 Summary / 选取部分历史记录
//!

pub trait History {
    fn add_message(&mut self, message: Message);

    /// 因为最后要发送这里给出的所有 Message, 所以这里直接返回 Vec<Message>，而不是提供一个迭代器接口。
    /// 使用迭代器并不会提供更好的性能
    fn get_history(&self) -> Vec<Message>;
}

impl History for Vec<Message> {
    fn add_message(&mut self, message: Message) {
        self.push(message);
    }

    fn get_history(&self) -> Vec<Message> {
        self.clone()
    }
}

use std::error::Error;

use crate::{request::*, response::*};
use reqwest::Client;
use serde_json::Value;

/// 一个简单的DeepSeek聊天客户端，使用Vec<Messages>存储对话历史记录
/// history 中的第一条消息是 System Prompt，后续消息是 User 或 Assistant 消息
pub struct NormalChatter {
    pub token: String,
    pub client: Client,
}

impl NormalChatter {
    pub fn new(token: String) -> Self {
        Self {
            token,
            client: Client::new(),
        }
    }

    pub async fn chat<T: AsRef<str>>(
        &mut self,
        user_message: T,
        history: &mut impl History,
    ) -> Result<String, Box<dyn Error>> {
        let user_message = Message::new(Role::User, user_message.as_ref());
        history.add_message(user_message);

        let response = Request::basic_query(history.get_history())
            .execute_nostreaming(&self.token)
            .await?;

        let assistant_message = response.choices[0].message.clone();
        history.add_message(assistant_message);

        Ok(response.content().to_string())
    }

    pub async fn chat_json<T: AsRef<str>>(
        &mut self,
        user_message: T,
        history: &mut impl History,
    ) -> Result<Value, Box<dyn Error>> {
        let user_message = Message::new(Role::User, user_message.as_ref());
        history.add_message(user_message);

        let response = Request::basic_query(history.get_history())
            .json()
            .execute_nostreaming(&self.token)
            .await?;

        let assistant_message = response.choices[0].message.clone();
        history.add_message(assistant_message);

        let value = serde_json::from_str(response.content())?;

        Ok(value)
    }
}
