use serde::{Deserialize, Serialize};

use super::message::ToolType;

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ToolChoice {
    String(ToolChoiceType),

    /// type string REQUIRED
    ///     Possible values: `function`
    ///     The tool type. Currently only `function` is supported.
    /// function object REQUIRED
    ///     name string REQUIRED
    ///     The name of the function to invoke.
    Object(ToolChoiceObject),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolChoiceType {
    None,
    Auto,
    Required,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolChoiceObject {
    pub r#type: ToolType,
    pub function: FunctionName,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FunctionName {
    pub name: String,
}
