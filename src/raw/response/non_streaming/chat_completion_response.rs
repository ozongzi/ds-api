use serde::Deserialize;

use super::{choice::Choice, object_type::ObjectType, usage::Usage};
use crate::raw::Model;

#[derive(Debug, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub choices: Vec<Choice>,
    pub created: u64,
    pub model: Model,
    pub system_fingerprint: String,
    #[serde(rename = "object")]
    pub object: ObjectType,
    pub usage: Usage,
}
