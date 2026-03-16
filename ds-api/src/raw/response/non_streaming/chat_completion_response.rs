use serde::Deserialize;

use super::{choice::Choice, object_type::ObjectType, usage::Usage};
use crate::raw::Model;

#[derive(Debug, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub choices: Vec<Choice>,
    pub created: u64,
    pub model: Model,
    #[serde(default)]
    pub system_fingerprint: Option<String>,
    #[serde(rename = "object")]
    pub object: ObjectType,
    pub usage: Usage,
}

impl ChatCompletionResponse {
    pub fn content(&self) -> Option<&str> {
        self.choices.first()?.message.content.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::ChatCompletionResponse;

    #[test]
    fn content_returns_first_choice_content() {
        let resp: ChatCompletionResponse = serde_json::from_value(serde_json::json!({
            "id": "cmpl_1",
            "choices": [
                {
                    "finish_reason": "stop",
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": "hello"
                    }
                }
            ],
            "created": 1,
            "model": "deepseek-chat",
            "object": "chat.completion",
            "usage": {
                "completion_tokens": 1,
                "prompt_tokens": 1,
                "total_tokens": 2
            }
        }))
        .unwrap();

        assert_eq!(resp.content(), Some("hello"));
    }

    #[test]
    fn content_returns_none_when_no_choices() {
        let resp: ChatCompletionResponse = serde_json::from_value(serde_json::json!({
            "id": "cmpl_2",
            "choices": [],
            "created": 1,
            "model": "deepseek-chat",
            "object": "chat.completion",
            "usage": {
                "completion_tokens": 0,
                "prompt_tokens": 1,
                "total_tokens": 1
            }
        }))
        .unwrap();

        assert_eq!(resp.content(), None);
    }
}
