use crate::raw::request::tool::Tool as RawTool;
use async_trait::async_trait;
use serde_json::Value;
use serde_json::json;
use std::collections::HashMap;

/// The core trait that all agent tools must implement.
///
/// You should not implement this trait manually. Instead, annotate your `impl` block
/// with the [`#[tool]`][ds_api_macros::tool] macro and write plain `async fn` methods —
/// the macro generates the `raw_tools` and `call` implementations for you.
///
/// # What the macro generates
///
/// For each `async fn` in the annotated `impl`:
/// - A [`RawTool`] entry (name, description from doc comment, JSON Schema from parameter types)
///   is added to the `raw_tools()` vec.
/// - A `match` arm in `call()` that deserialises each argument from the incoming `args` JSON,
///   invokes the method, and serialises the return value via `serde_json::to_value`.
///
/// Any return type that implements `serde::Serialize` is accepted — `serde_json::Value`,
/// plain structs with `#[derive(Serialize)]`, primitives, `Option<T>`, `Vec<T>`, etc.
///
/// # Example
///
/// ```no_run
/// use ds_api::{DeepseekAgent, tool};
/// use serde_json::{Value, json};
///
/// struct Calc;
///
/// #[tool]
/// impl ds_api::Tool for Calc {
///     /// Add two integers together.
///     /// a: first operand
///     /// b: second operand
///     async fn add(&self, a: i64, b: i64) -> i64 {
///         a + b
///     }
/// }
///
/// # #[tokio::main] async fn main() {
/// let agent = DeepseekAgent::new("sk-...").add_tool(Calc);
/// # }
/// ```
#[async_trait]
pub trait Tool: Send + Sync {
    /// Return the list of raw tool definitions to send to the API.
    fn raw_tools(&self) -> Vec<RawTool>;

    /// Invoke the named tool with the given arguments and return the result as a JSON value.
    ///
    /// When using the `#[tool]` macro you do not implement this method yourself —
    /// the macro generates it. The generated implementation accepts any return type
    /// that implements `serde::Serialize` (including `serde_json::Value`, plain
    /// structs with `#[derive(Serialize)]`, primitives, etc.) and converts the
    /// value to `serde_json::Value` automatically.
    async fn call(&self, name: &str, args: Value) -> Value;
}

/// 将多个 Tool 合并为一个，方便批量注册进 agent。
pub struct ToolBundle {
    tools: Vec<Box<dyn Tool>>,
    index: std::collections::HashMap<String, usize>,
}

impl ToolBundle {
    pub fn new() -> Self {
        Self {
            tools: vec![],
            index: HashMap::new(),
        }
    }

    pub fn add<T: Tool + 'static>(mut self, tool: T) -> Self {
        let idx = self.tools.len();
        for raw in tool.raw_tools() {
            self.index.insert(raw.function.name.clone(), idx);
        }
        self.tools.push(Box::new(tool));
        self
    }
}

#[async_trait]
impl Tool for ToolBundle {
    fn raw_tools(&self) -> Vec<RawTool> {
        self.tools.iter().flat_map(|t| t.raw_tools()).collect()
    }

    async fn call(&self, name: &str, args: Value) -> Value {
        match self.index.get(name) {
            Some(&idx) => self.tools[idx].call(name, args).await,
            None => json!({ "error": format!("未知工具: {name}") }),
        }
    }
}
