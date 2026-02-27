use ds_api::error::Result;
use ds_api::raw::Message as RawMessage;
use ds_api::request::Request;

use futures::StreamExt;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_execute_nostreaming_success() -> Result<()> {
    // Start a mock server
    let mock_server = MockServer::start().await;

    // Mock response body for non-streaming request
    let body = serde_json::json!({
        "id": "1",
        "object": "chat.completion",
        "created": 1u64,
        "model": "deepseek-chat",
        "system_fingerprint": "fp",
        "choices": [
            {
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Hello from mock"
                },
                "finish_reason": "stop"
            }
        ],
        "usage": {
            "prompt_tokens": 1,
            "completion_tokens": 1,
            "total_tokens": 2
        }
    });

    // Expect a POST to /v1/chat/completions with Bearer header
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    let req = Request::basic_query(vec![RawMessage::new(
        ds_api::raw::request::message::Role::User,
        "hi",
    )]);

    // Point to mock server base (no /v1)
    let base = mock_server.uri();
    let resp = req
        .execute_client_baseurl_nostreaming(&client, &base, "test-token")
        .await?;

    assert_eq!(resp.choices.len(), 1);
    assert_eq!(
        resp.choices[0].message.content.as_deref(),
        Some("Hello from mock")
    );

    Ok(())
}

#[tokio::test]
async fn test_execute_client_streaming_baseurl_parses_chunks() -> Result<()> {
    // Start mock server
    let mock_server = MockServer::start().await;

    // Prepare SSE body (two JSON data events and a [DONE])
    let sse_body = "data: {\"id\":\"c1\",\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n"
        .to_string()
        + "data: {\"id\":\"c2\",\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n"
        + "data: [DONE]\n\n";

    // Mock SSE endpoint
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer test-token"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_header("content-type", "text/event-stream")
                .set_body_raw(sse_body, "text/event-stream"),
        )
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    let req = Request::basic_query(vec![RawMessage::new(
        ds_api::raw::request::message::Role::User,
        "hi",
    )]);

    // call streaming with the mock server base
    let base = mock_server.uri();
    let stream = req
        .execute_client_baseurl_streaming(&client, &base, "test-token")
        .await?;

    futures::pin_mut!(stream);

    let mut collected = String::new();

    // Collect events from the stream
    while let Some(item) = stream.next().await {
        let chunk = item?;
        // Match on the streaming chunk structure and append delta content if present
        match chunk {
            ds_api::raw::response::streaming::ChatCompletionChunk { choices, .. } => {
                if let Some(delta) = choices.get(0).and_then(|c| c.delta.content.as_ref()) {
                    collected.push_str(delta);
                }
            }
        }
    }

    assert_eq!(collected, "hello world");

    Ok(())
}
