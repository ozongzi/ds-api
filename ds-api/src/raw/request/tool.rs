use serde::{Deserialize, Serialize};

use super::message::ToolType;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Tool {
    /// The tool's type. Currently only `function` is supported.
    pub r#type: ToolType,
    pub function: Function,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Function {
    /// The name of the function to call. Allowed characters: a-z, A-Z, 0-9, underscore and hyphen.
    /// Maximum length is 64 characters.
    pub name: String,

    /// A description of the function's behavior to help the model decide when and how to call it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// The function's input parameters described as a JSON Schema object.
    /// See the Tool Calls guide for examples and the JSON Schema reference for format details.
    /// Omitting `parameters` defines a function that takes an empty parameter list.
    pub parameters: serde_json::Value,

    /// Beta feature: when true, the API will use strict mode for function calls.
    /// In strict mode, outputs are validated against the function's JSON schema to ensure compliance.
    /// This is a beta feature; consult the Tool Calls guide for usage details.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strict: Option<bool>,
}
