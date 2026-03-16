use async_trait::async_trait;
use ds_api::raw::{Function, Tool, ToolType, request::stop::Stop};
use ds_api::{ApiError, Tool as ToolTrait, ToolBundle};
use serde_json::{Value, json};

struct EchoTool;

#[async_trait]
impl ToolTrait for EchoTool {
    fn raw_tools(&self) -> Vec<Tool> {
        vec![Tool {
            r#type: ToolType::Function,
            function: Function {
                name: "echo".to_string(),
                description: Some("Echo input".to_string()),
                parameters: json!({"type":"object","properties":{"value":{"type":"string"}}}),
                strict: Some(false),
            },
        }]
    }

    async fn call(&self, name: &str, args: Value) -> Value {
        json!({"name": name, "args": args})
    }
}

#[tokio::test]
async fn tool_bundle_dispatches_and_handles_missing() {
    let bundle = ToolBundle::new().add(EchoTool);

    let known = bundle.call("echo", json!({"value":"ok"})).await;
    assert_eq!(known["name"], "echo");
    assert_eq!(known["args"]["value"], "ok");

    let unknown = bundle.call("not_found", json!({})).await;
    assert!(unknown["error"].as_str().unwrap().contains("未知工具"));

    let defs = bundle.raw_tools();
    assert_eq!(defs.len(), 1);
    assert_eq!(defs[0].function.name, "echo");
}

#[test]
fn api_error_conversions_and_http_constructor() {
    let http = ApiError::http_error(reqwest::StatusCode::BAD_REQUEST, "bad");
    match http {
        ApiError::Http { status, text } => {
            assert_eq!(status, reqwest::StatusCode::BAD_REQUEST);
            assert_eq!(text, "bad");
        }
        _ => panic!("unexpected variant"),
    }

    let from_str: ApiError = "oops".into();
    let from_string: ApiError = String::from("oops2").into();

    assert!(matches!(from_str, ApiError::Other(ref s) if s == "oops"));
    assert!(matches!(from_string, ApiError::Other(ref s) if s == "oops2"));
}

#[test]
fn stop_enum_serializes_and_deserializes_both_shapes() {
    let stop_string = Stop::String("END".to_string());
    let v1 = serde_json::to_value(&stop_string).unwrap();
    assert_eq!(v1, json!("END"));
    let back1: Stop = serde_json::from_value(v1).unwrap();
    assert!(matches!(back1, Stop::String(s) if s == "END"));

    let stop_array = Stop::Array(vec!["A".into(), "B".into()]);
    let v2 = serde_json::to_value(&stop_array).unwrap();
    assert_eq!(v2, json!(["A", "B"]));
    let back2: Stop = serde_json::from_value(v2).unwrap();
    assert!(matches!(back2, Stop::Array(arr) if arr == vec!["A", "B"]));
}
