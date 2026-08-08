#![cfg(feature = "openai")]

use async_llm::openai::{
    ChatCompletionError, ChatCompletionRequest, ChatMessage, Client, StreamOptions, ToolChoice,
};
use tokio_stream::StreamExt;
use wiremock::{
    matchers::{header, method, path},
    Mock, MockServer, ResponseTemplate,
};

fn stream_request() -> ChatCompletionRequest {
    ChatCompletionRequest {
        model: "mock-model".into(),
        messages: vec![ChatMessage::user("hello")],
        stream: true,
        stream_options: Some(StreamOptions {
            include_usage: true,
        }),
        ..Default::default()
    }
}

#[test]
fn serializes_chat_completion_stream_request() {
    let value = serde_json::to_value(stream_request()).unwrap();

    assert_eq!(value["stream"], true);
    assert_eq!(value["stream_options"]["include_usage"], true);
}

#[test]
fn serializes_tool_choice_selectors() {
    let cases = [
        (ToolChoice::Auto, serde_json::json!("auto")),
        (ToolChoice::Required, serde_json::json!("required")),
        (
            ToolChoice::Function {
                name: "weather".into(),
            },
            serde_json::json!({"type": "function", "function": {"name": "weather"}}),
        ),
    ];

    for (tool_choice, expected) in cases {
        let request = ChatCompletionRequest {
            tool_choice: Some(tool_choice),
            ..stream_request()
        };

        assert_eq!(
            serde_json::to_value(request).unwrap()["tool_choice"],
            expected
        );
    }
}

#[test]
fn deserializes_chunk_tool_reasoning_and_cached_usage() {
    let chunk = serde_json::from_str::<async_llm::openai::ChatCompletionChunk>(
        r#"{
            "choices": [{
                "delta": {
                    "content": "answer",
                    "reasoning_content": "first reasoning",
                    "reasoning": "second reasoning",
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_1",
                        "function": {"name": "weather", "arguments": "{\"city\":"}
                    }]
                },
                "finish_reason": "length"
            }],
            "usage": {
                "prompt_tokens": 21,
                "completion_tokens": 8,
                "prompt_tokens_details": {"cached_tokens": 13}
            }
        }"#,
    )
    .unwrap();

    let choice = &chunk.choices[0];
    assert_eq!(choice.delta.reasoning_trace(), Some("first reasoning"));
    assert_eq!(
        choice.delta.tool_calls.as_ref().unwrap()[0].id.as_deref(),
        Some("call_1")
    );
    assert_eq!(choice.finish_reason.as_deref(), Some("length"));
    assert_eq!(chunk.usage.unwrap().cached_tokens(), Some(13));

    let fallback =
        serde_json::from_str::<async_llm::openai::Delta>(r#"{"reasoning":"fallback"}"#).unwrap();
    assert_eq!(fallback.reasoning_trace(), Some("fallback"));
}

#[tokio::test]
async fn streams_fragmented_tool_calls_without_merging_them() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"data: {"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"weather","arguments":"{\"city\":"}}]}}]}

data: {"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"Paris\"}"}}]}}]}

data: [DONE]

"#,
            "text/event-stream",
        ))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::builder().base_url(server.uri()).build().unwrap();
    let chunks = client
        .stream(stream_request())
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert_eq!(chunks.len(), 2);
    let first_tool = &chunks[0].as_ref().unwrap().choices[0]
        .delta
        .tool_calls
        .as_ref()
        .unwrap()[0];
    assert_eq!(first_tool.id.as_deref(), Some("call_1"));
    assert_eq!(
        first_tool.function.as_ref().unwrap().arguments.as_deref(),
        Some("{\"city\":")
    );
    let second_tool = &chunks[1].as_ref().unwrap().choices[0]
        .delta
        .tool_calls
        .as_ref()
        .unwrap()[0];
    assert_eq!(second_tool.index, 0);
    assert_eq!(
        second_tool.function.as_ref().unwrap().arguments.as_deref(),
        Some("\"Paris\"}")
    );
}

#[tokio::test]
async fn streams_sse_chunks_with_bearer_auth_and_done_terminal() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"data: {"choices":[{"delta":{"content":"hello"}}]}

data: [DONE]

"#,
            "text/event-stream",
        ))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::builder()
        .api_key("test-key")
        .base_url(server.uri())
        .build()
        .unwrap();
    let chunks = client
        .stream(stream_request())
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert_eq!(chunks.len(), 1);
    assert_eq!(
        chunks[0].as_ref().unwrap().choices[0]
            .delta
            .content
            .as_deref(),
        Some("hello")
    );
}

#[tokio::test]
async fn classifies_rate_limits_and_captures_the_response_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string("slow down"))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::builder()
        .base_url(server.uri())
        .max_retries(0)
        .build()
        .unwrap();
    let result = client
        .stream(stream_request())
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert!(matches!(
        result.as_slice(),
        [Err(ChatCompletionError::RateLimited { body })] if body == "slow down"
    ));
}

#[tokio::test]
async fn reports_a_stream_without_a_terminal_frame_as_incomplete() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"choices\":[{\"delta\":{\"content\":\"partial\"}}]}\n\n",
            "text/event-stream",
        ))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::builder().base_url(server.uri()).build().unwrap();
    let result = client
        .stream(stream_request())
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert!(matches!(
        result.as_slice(),
        [Ok(_), Err(ChatCompletionError::IncompleteStream)]
    ));
}
