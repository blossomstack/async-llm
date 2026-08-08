#![cfg(feature = "mock")]

use async_llm::{
    mock::MockLlmServer,
    openai::{ChatCompletionRequest, ChatMessage, Client as OpenAiClient, StreamOptions},
    responses::{Client as ResponsesClient, ResponsesRequest, ResponsesStreamEvent},
};
use tokio_stream::StreamExt;

#[tokio::test]
async fn one_server_serves_all_protocol_routes_in_fifo_order() {
    let server = MockLlmServer::builder().build().await;
    server.queue_response("anthropic response");
    server.queue_response("chat completion response");
    server.queue_response("responses response");

    let client = reqwest::Client::new();
    let anthropic = client
        .post(format!("{}/v1/messages", server.url()))
        .json(&serde_json::json!({
            "model": "mock-model",
            "max_tokens": 1,
            "messages": [],
            "stream": true,
        }))
        .send()
        .await
        .unwrap();
    assert!(anthropic.status().is_success());
    let anthropic_body = anthropic.text().await.unwrap();
    assert!(anthropic_body.contains("anthropic response"));
    assert!(anthropic_body.contains("event: message_stop"));

    let chat = client
        .post(format!("{}/v1/chat/completions", server.url()))
        .json(&serde_json::json!({
            "model": "mock-model",
            "messages": [],
            "stream": true,
        }))
        .send()
        .await
        .unwrap();
    assert!(chat.status().is_success());
    let chat_body = chat.text().await.unwrap();
    assert!(chat_body.contains("chat completion response"));
    assert!(chat_body.contains("data: [DONE]"));

    let responses = client
        .post(format!("{}/responses", server.url()))
        .json(&serde_json::json!({
            "model": "mock-model",
            "input": [],
            "stream": true,
            "store": false,
        }))
        .send()
        .await
        .unwrap();
    assert!(responses.status().is_success());
    let responses_body = responses.text().await.unwrap();
    assert!(responses_body.contains("responses response"));
    assert!(responses_body.contains("event: response.completed"));

    assert_eq!(server.queued_count(), 0);
}

#[tokio::test]
async fn openai_client_consumes_mock_stream_and_terminal_chunk() {
    let server = MockLlmServer::builder()
        .response("openai response")
        .build()
        .await;
    let request = ChatCompletionRequest {
        model: "mock-model".into(),
        messages: vec![ChatMessage::user("hello")],
        stream: true,
        stream_options: Some(StreamOptions {
            include_usage: true,
        }),
        ..Default::default()
    };

    let chunks = OpenAiClient::builder()
        .base_url(server.url())
        .build()
        .unwrap()
        .stream(request)
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert!(matches!(
        chunks.as_slice(),
        [Ok(content), Ok(terminal)]
            if content.choices[0].delta.content.as_deref() == Some("openai response")
                && terminal.choices[0].finish_reason.as_deref() == Some("stop")
    ));
}

#[tokio::test]
async fn responses_client_consumes_mock_stream_and_completed_terminal() {
    let server = MockLlmServer::builder()
        .response("responses client response")
        .build()
        .await;

    let events = ResponsesClient::with_api_key("test-key")
        .with_base_url(server.url())
        .stream(ResponsesRequest::for_text("mock-model", "hello"))
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert!(matches!(
        events.as_slice(),
        [
            Ok(ResponsesStreamEvent::Other { event_type, .. }),
            Ok(ResponsesStreamEvent::OutputItemAdded { .. }),
            Ok(ResponsesStreamEvent::OutputTextDelta { delta, .. }),
            Ok(ResponsesStreamEvent::OutputItemDone { .. }),
            Ok(ResponsesStreamEvent::Completed { .. }),
        ] if event_type == "response.created" && delta == "responses client response"
    ));
}
