use crate::raw::request::tool::Tool as RawTool;
use async_trait::async_trait;
use serde_json::{Value, json};

pub trait JsonSchema {
    fn schema() -> Value;
}

impl JsonSchema for String {
    fn schema() -> Value {
        json!({"type": "string"})
    }
}
impl JsonSchema for &str {
    fn schema() -> Value {
        json!({"type": "string"})
    }
}
impl JsonSchema for bool {
    fn schema() -> Value {
        json!({"type": "boolean"})
    }
}
impl JsonSchema for i8 {
    fn schema() -> Value {
        json!({"type": "integer"})
    }
}
impl JsonSchema for i16 {
    fn schema() -> Value {
        json!({"type": "integer"})
    }
}
impl JsonSchema for i32 {
    fn schema() -> Value {
        json!({"type": "integer"})
    }
}
impl JsonSchema for i64 {
    fn schema() -> Value {
        json!({"type": "integer"})
    }
}
impl JsonSchema for u8 {
    fn schema() -> Value {
        json!({"type": "integer"})
    }
}
impl JsonSchema for u16 {
    fn schema() -> Value {
        json!({"type": "integer"})
    }
}
impl JsonSchema for u32 {
    fn schema() -> Value {
        json!({"type": "integer"})
    }
}
impl JsonSchema for u64 {
    fn schema() -> Value {
        json!({"type": "integer"})
    }
}
impl JsonSchema for f32 {
    fn schema() -> Value {
        json!({"type": "number"})
    }
}
impl JsonSchema for f64 {
    fn schema() -> Value {
        json!({"type": "number"})
    }
}

impl<T: JsonSchema> JsonSchema for Vec<T> {
    fn schema() -> Value {
        json!({"type": "array", "items": T::schema()})
    }
}

impl<T: JsonSchema> JsonSchema for Option<T> {
    fn schema() -> Value {
        T::schema()
    }
}

#[async_trait]
pub trait Tool: Send + Sync {
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
