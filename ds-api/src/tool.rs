use async_trait::async_trait;
use serde_json::{json, Value};
use crate::raw::request::tool::Tool as RawTool;

pub trait JsonSchema {
    fn schema() -> Value;
}

impl JsonSchema for String  { fn schema() -> Value { json!({"type": "string"}) } }
impl JsonSchema for &str    { fn schema() -> Value { json!({"type": "string"}) } }
impl JsonSchema for bool    { fn schema() -> Value { json!({"type": "boolean"}) } }
impl JsonSchema for i8      { fn schema() -> Value { json!({"type": "integer"}) } }
impl JsonSchema for i16     { fn schema() -> Value { json!({"type": "integer"}) } }
impl JsonSchema for i32     { fn schema() -> Value { json!({"type": "integer"}) } }
impl JsonSchema for i64     { fn schema() -> Value { json!({"type": "integer"}) } }
impl JsonSchema for u8      { fn schema() -> Value { json!({"type": "integer"}) } }
impl JsonSchema for u16     { fn schema() -> Value { json!({"type": "integer"}) } }
impl JsonSchema for u32     { fn schema() -> Value { json!({"type": "integer"}) } }
impl JsonSchema for u64     { fn schema() -> Value { json!({"type": "integer"}) } }
impl JsonSchema for f32     { fn schema() -> Value { json!({"type": "number"}) } }
impl JsonSchema for f64     { fn schema() -> Value { json!({"type": "number"}) } }

impl<T: JsonSchema> JsonSchema for Vec<T> {
    fn schema() -> Value { json!({"type": "array", "items": T::schema()}) }
}

impl<T: JsonSchema> JsonSchema for Option<T> {
    fn schema() -> Value { T::schema() }
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn raw_tools(&self) -> Vec<RawTool>;
    async fn call(&self, name: &str, args: Value) -> Value;
}
