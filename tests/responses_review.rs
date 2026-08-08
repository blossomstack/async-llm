#![cfg(feature = "responses")]

use async_llm::responses::{chatgpt::StoredTokens, ResponsesRequest, ResponsesStreamEvent};

#[test]
fn failed_response_and_error_events_preserve_server_details() {
    let failed: ResponsesStreamEvent = serde_json::from_str(
        r#"{"type":"response.failed","response":{"status":"failed","error":{"code":"server_error","message":"upstream unavailable","param":"model"}}}"#,
    )
    .unwrap();
    assert!(matches!(
        failed,
        ResponsesStreamEvent::Failed { response }
            if response.status.as_deref() == Some("failed")
                && response.error.as_ref().is_some_and(|error|
                    error.code.as_deref() == Some("server_error")
                        && error.message.as_deref() == Some("upstream unavailable")
                        && error.param.as_deref() == Some("model"))
    ));

    let error: ResponsesStreamEvent = serde_json::from_str(
        r#"{"type":"error","code":"invalid_request_error","message":"bad request","param":"input"}"#,
    )
    .unwrap();
    assert!(matches!(
        error,
        ResponsesStreamEvent::Error { code, message, param }
            if code == "invalid_request_error" && message == "bad request" && param.as_deref() == Some("input")
    ));
}

#[tokio::test]
async fn error_event_is_terminal_without_an_incomplete_stream_error() {
    use async_llm::responses::Client;
    use tokio_stream::StreamExt;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"type\":\"error\",\"code\":\"server_error\",\"message\":\"unavailable\"}\n\n",
            "text/event-stream",
        ))
        .mount(&server)
        .await;

    let events = Client::with_api_key("test-key")
        .with_base_url(server.uri())
        .stream(ResponsesRequest::for_text("gpt-5", "hello"))
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert!(matches!(
        events.as_slice(),
        [Ok(ResponsesStreamEvent::Error { code, message, .. })]
            if code == "server_error" && message == "unavailable"
    ));
}

#[tokio::test]
async fn failed_event_is_terminal_without_an_incomplete_stream_error() {
    use async_llm::responses::Client;
    use tokio_stream::StreamExt;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"type\":\"response.failed\",\"response\":{\"status\":\"failed\",\"error\":{\"code\":\"server_error\",\"message\":\"unavailable\"}}}\n\n",
            "text/event-stream",
        ))
        .mount(&server)
        .await;

    let events = Client::with_api_key("test-key")
        .with_base_url(server.uri())
        .stream(ResponsesRequest::for_text("gpt-5", "hello"))
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert!(matches!(
        events.as_slice(),
        [Ok(ResponsesStreamEvent::Failed { response })]
            if response.status.as_deref() == Some("failed")
                && response.error.as_ref().is_some_and(|error| error.message.as_deref() == Some("unavailable"))
    ));
}

#[test]
fn final_refusal_and_unrecognized_events_preserve_protocol_data() {
    let text: ResponsesStreamEvent = serde_json::from_str(
        r#"{"type":"response.output_text.done","item_id":"msg_1","output_index":0,"content_index":0,"text":"complete"}"#,
    )
    .unwrap();
    assert!(matches!(
        text,
        ResponsesStreamEvent::OutputTextDone { text, .. } if text == "complete"
    ));

    let arguments: ResponsesStreamEvent = serde_json::from_str(
        r#"{"type":"response.function_call_arguments.done","item_id":"fc_1","output_index":0,"call_id":"call_1","arguments":"{\"city\":\"Paris\"}"}"#,
    )
    .unwrap();
    assert!(matches!(
        arguments,
        ResponsesStreamEvent::FunctionCallArgumentsDone { arguments, .. }
            if arguments == "{\"city\":\"Paris\"}"
    ));

    let refusal: ResponsesStreamEvent = serde_json::from_str(
        r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"message","content":[{"type":"refusal","refusal":"I cannot help with that."}]}}"#,
    )
    .unwrap();
    assert!(matches!(
        refusal,
        ResponsesStreamEvent::OutputItemDone { item, .. }
            if item.content[0].refusal.as_deref() == Some("I cannot help with that.")
    ));

    let unknown: ResponsesStreamEvent = serde_json::from_str(
        r#"{"type":"response.future.event","sequence_number":42,"payload":{"new_field":true}}"#,
    )
    .unwrap();
    assert!(matches!(
        unknown,
        ResponsesStreamEvent::Other { event_type, data }
            if event_type == "response.future.event"
                && data["sequence_number"] == 42
                && data["payload"]["new_field"] == true
    ));
}

#[tokio::test]
async fn refresh_response_without_id_or_refresh_tokens_keeps_stored_identity() {
    use async_llm::responses::chatgpt::{ChatGptAuth, ChatGptTokens, TokenStore};
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use wiremock::{
        matchers::{body_string, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[derive(Default)]
    struct Store(Mutex<Vec<StoredTokens>>);
    #[async_trait]
    impl TokenStore for Store {
        async fn load(&self) -> Result<Option<StoredTokens>, async_llm::responses::ResponsesError> {
            Ok(None)
        }
        async fn save(
            &self,
            tokens: StoredTokens,
        ) -> Result<(), async_llm::responses::ResponsesError> {
            self.0.lock().unwrap().push(tokens);
            Ok(())
        }
    }

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string(
            "grant_type=refresh_token&refresh_token=stored-refresh&client_id=test-client",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "replacement-access"
        })))
        .mount(&server)
        .await;
    let store = Arc::new(Store::default());
    let tokens = ChatGptTokens::new(
        StoredTokens {
            access_token: "old-access".into(),
            refresh_token: "stored-refresh".into(),
            id_token: Some("stored-id".into()),
            account_id: Some("stored-account".into()),
            expires_at: None,
        },
        store.clone(),
        ChatGptAuth::new("test-client").with_issuer(server.uri()),
    );
    tokens.refresh().await.unwrap();
    assert_eq!(tokens.access_token().await.unwrap(), "replacement-access");
    assert_eq!(
        tokens.account_id().await.unwrap().as_deref(),
        Some("stored-account")
    );
    let persisted = store.0.lock().unwrap()[0].clone();
    assert_eq!(persisted.refresh_token, "stored-refresh");
    assert_eq!(persisted.id_token.as_deref(), Some("stored-id"));
    assert_eq!(persisted.account_id.as_deref(), Some("stored-account"));
}

#[test]
fn stored_tokens_debug_redacts_all_token_secrets() {
    let tokens = StoredTokens {
        access_token: "access-secret".into(),
        refresh_token: "refresh-secret".into(),
        id_token: Some("id-secret".into()),
        account_id: Some("account".into()),
        expires_at: None,
    };
    let debug = format!("{tokens:?}");

    assert!(!debug.contains("access-secret"));
    assert!(!debug.contains("refresh-secret"));
    assert!(!debug.contains("id-secret"));
}
