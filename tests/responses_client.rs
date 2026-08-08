#![cfg(feature = "responses")]

use async_llm::responses::{
    FunctionTool, ReasoningControl, ResponsesRequest, ResponsesStreamEvent,
};

#[test]
fn responses_request_serializes_stream_and_store_flags() {
    let request = ResponsesRequest::for_text("gpt-5", "hello");
    let json = serde_json::to_value(request).unwrap();

    assert_eq!(json["stream"], true);
    assert_eq!(json["store"], false);
}

#[test]
fn incomplete_event_preserves_max_output_tokens_reason() {
    let event: ResponsesStreamEvent = serde_json::from_str(
        r#"{"type":"response.incomplete","response":{"incomplete_details":{"reason":"max_output_tokens"}}}"#,
    )
    .unwrap();

    assert!(event.is_max_output_tokens());
}

#[test]
fn responses_request_keeps_native_input_tools_and_reasoning_controls() {
    let request = ResponsesRequest::new(
        "gpt-5",
        vec![serde_json::json!({"role": "assistant", "content": []})],
    )
    .with_function_tool(FunctionTool::new(
        "weather",
        "Looks up the weather",
        serde_json::json!({"type": "object", "properties": {}}),
    ))
    .with_reasoning(ReasoningControl::new("high"));

    let json = serde_json::to_value(request).unwrap();
    assert_eq!(json["input"][0]["role"], "assistant");
    assert_eq!(json["tools"][0]["type"], "function");
    assert_eq!(json["tools"][0]["name"], "weather");
    assert_eq!(json["reasoning"]["effort"], "high");
}

#[test]
fn response_events_deserialize_text_function_and_encrypted_reasoning_deltas() {
    let text: ResponsesStreamEvent = serde_json::from_str(
        r#"{"type":"response.output_text.delta","item_id":"msg_1","output_index":0,"content_index":0,"delta":"hello"}"#,
    )
    .unwrap();
    assert!(matches!(
        text,
        ResponsesStreamEvent::OutputTextDelta { ref delta, .. } if delta == "hello"
    ));

    let function: ResponsesStreamEvent = serde_json::from_str(
        r#"{"type":"response.function_call_arguments.delta","item_id":"fc_1","output_index":0,"delta":"{\"city\":"}"#,
    )
    .unwrap();
    assert!(matches!(
        function,
        ResponsesStreamEvent::FunctionCallArgumentsDelta { ref delta, .. } if delta == "{\"city\":"
    ));

    let reasoning: ResponsesStreamEvent = serde_json::from_str(
        r#"{"type":"response.reasoning.encrypted_content","item_id":"rs_1","output_index":0,"content_index":0,"encrypted_content":"opaque"}"#,
    )
    .unwrap();
    assert!(matches!(
        reasoning,
        ResponsesStreamEvent::ReasoningEncryptedContent { ref encrypted_content, .. }
            if encrypted_content == "opaque"
    ));
}

#[tokio::test]
async fn api_key_client_streams_responses_events_and_requires_a_terminal_frame() {
    use async_llm::responses::{Client, ResponsesError};
    use tokio_stream::StreamExt;
    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .and(header("authorization", "Bearer test-key"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"hello\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{}}\n\n",
            "text/event-stream",
        ))
        .expect(1)
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
        [Ok(ResponsesStreamEvent::OutputTextDelta { delta, .. }), Ok(ResponsesStreamEvent::Completed { .. })]
            if delta == "hello"
    ));

    let cut_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"type\":\"response.output_text.delta\",\"item_id\":\"msg_1\",\"output_index\":0,\"content_index\":0,\"delta\":\"partial\"}\n\n",
            "text/event-stream",
        ))
        .mount(&cut_server)
        .await;

    let cut_events = Client::with_api_key("test-key")
        .with_base_url(cut_server.uri())
        .stream(ResponsesRequest::for_text("gpt-5", "hello"))
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;
    assert!(matches!(
        cut_events.as_slice(),
        [Ok(_), Err(ResponsesError::IncompleteStream)]
    ));
}

#[tokio::test]
async fn client_classifies_responses_http_statuses() {
    use async_llm::responses::{Client, ResponsesError};
    use tokio_stream::StreamExt;
    use wiremock::{matchers::method, Mock, MockServer, ResponseTemplate};

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(429).set_body_string("slow down"))
        .mount(&server)
        .await;

    let result = Client::with_api_key("test-key")
        .with_base_url(server.uri())
        .max_retries(0)
        .stream(ResponsesRequest::for_text("gpt-5", "hello"))
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert!(matches!(
        result.as_slice(),
        [Err(ResponsesError::RateLimited { body })] if body == "slow down"
    ));
}

#[tokio::test]
async fn chatgpt_refresh_extracts_preferred_account_and_persists_tokens() {
    use async_llm::responses::chatgpt::{ChatGptTokens, StoredTokens, TokenStore};
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use wiremock::{
        matchers::{body_json, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    #[derive(Default)]
    struct RecordingStore(Mutex<Vec<StoredTokens>>);

    #[async_trait]
    impl TokenStore for RecordingStore {
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

    let id_token = "eyJhbGciOiJub25lIn0.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjdF9wcmVmZXJyZWQifSwiYWNjb3VudF9pZCI6ImFjY3RfZmFsbGJhY2sifQ.";
    let stored = StoredTokens::new("old-access", "refresh", id_token).unwrap();
    assert_eq!(stored.account_id.as_deref(), Some("acct_preferred"));

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_json(serde_json::json!({
            "grant_type": "refresh_token",
            "refresh_token": "refresh",
            "client_id": "test-client"
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "new-access",
            "refresh_token": "new-refresh",
            "id_token": id_token
        })))
        .mount(&server)
        .await;

    let store = Arc::new(RecordingStore::default());
    let tokens =
        ChatGptTokens::with_store_and_issuer(stored, store.clone(), server.uri(), "test-client");
    tokens.refresh().await.unwrap();

    assert_eq!(tokens.access_token().await.unwrap(), "new-access");
    assert_eq!(store.0.lock().unwrap().as_slice().len(), 1);
}

#[test]
fn device_login_serializes_authorization_and_poll_requests() {
    use async_llm::responses::chatgpt::DeviceLogin;

    let login = DeviceLogin::new("device_1", "ABCD", "https://auth.openai.com/activate", 5);
    assert_eq!(
        login.authorization_request("test-client"),
        serde_json::json!({"client_id": "test-client"})
    );
    assert_eq!(
        login.poll_request(),
        serde_json::json!({"device_auth_id": "device_1", "user_code": "ABCD"})
    );
}

#[tokio::test]
async fn device_login_start_and_poll_use_native_auth_payloads() {
    use async_llm::responses::chatgpt::{
        poll_device_login, start_device_login, StoredTokens, TokenStore,
    };
    use async_trait::async_trait;
    use std::sync::Arc;
    use wiremock::{
        matchers::{body_json, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    struct EmptyStore;

    #[async_trait]
    impl TokenStore for EmptyStore {
        async fn load(&self) -> Result<Option<StoredTokens>, async_llm::responses::ResponsesError> {
            Ok(None)
        }

        async fn save(&self, _: StoredTokens) -> Result<(), async_llm::responses::ResponsesError> {
            Ok(())
        }
    }

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/usercode"))
        .and(body_json(serde_json::json!({"client_id": "test-client"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "device_auth_id": "device_1",
            "user_code": "ABCD",
            "verification_uri": "https://auth.openai.com/activate",
            "interval": 5
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/token"))
        .and(body_json(
            serde_json::json!({"device_auth_id": "device_1", "user_code": "ABCD"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access",
            "refresh_token": "refresh"
        })))
        .mount(&server)
        .await;

    let http_client = reqwest::Client::new();
    let login = start_device_login(&http_client, server.uri(), "test-client")
        .await
        .unwrap();
    let tokens = poll_device_login(
        &http_client,
        server.uri(),
        "test-client",
        &login,
        Arc::new(EmptyStore),
    )
    .await
    .unwrap();

    assert_eq!(tokens.access_token().await.unwrap(), "access");
}

#[tokio::test]
async fn chatgpt_client_uses_codex_endpoint_and_account_header() {
    use async_llm::responses::{
        chatgpt::{ChatGptTokens, StoredTokens, TokenStore},
        Client, Credential,
    };
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio_stream::StreamExt;
    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    struct EmptyStore;

    #[async_trait]
    impl TokenStore for EmptyStore {
        async fn load(&self) -> Result<Option<StoredTokens>, async_llm::responses::ResponsesError> {
            Ok(None)
        }

        async fn save(&self, _: StoredTokens) -> Result<(), async_llm::responses::ResponsesError> {
            Ok(())
        }
    }

    let id_token = "eyJhbGciOiJub25lIn0.eyJodHRwczovL2FwaS5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjdF8xIn19.";
    let tokens = Arc::new(ChatGptTokens::with_store_and_issuer(
        StoredTokens::new("access", "refresh", id_token).unwrap(),
        Arc::new(EmptyStore),
        "https://auth.openai.com",
        "test-client",
    ));
    assert!(matches!(
        Credential::ChatGpt(tokens.clone()),
        Credential::ChatGpt(_)
    ));

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/backend-api/codex/responses"))
        .and(header("authorization", "Bearer access"))
        .and(header("chatgpt-account-id", "acct_1"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "data: {\"type\":\"response.completed\",\"response\":{}}\n\n",
            "text/event-stream",
        ))
        .mount(&server)
        .await;

    let events = Client::with_chatgpt(tokens)
        .with_base_url(server.uri())
        .stream(ResponsesRequest::for_text("gpt-5", "hello"))
        .await
        .unwrap()
        .collect::<Vec<_>>()
        .await;

    assert!(matches!(
        events.as_slice(),
        [Ok(ResponsesStreamEvent::Completed { .. })]
    ));
}

#[test]
fn output_item_and_reasoning_summary_events_keep_native_fields() {
    let output: ResponsesStreamEvent = serde_json::from_str(
        r#"{"type":"response.output_item.done","output_index":0,"item":{"id":"fc_1","type":"function_call","call_id":"call_1","name":"weather","arguments":"{\"city\":\"Paris\"}"}}"#,
    )
    .unwrap();
    assert!(matches!(
        output,
        ResponsesStreamEvent::OutputItemDone { item, .. }
            if item.call_id.as_deref() == Some("call_1")
                && item.arguments.as_deref() == Some("{\"city\":\"Paris\"}")
    ));

    let summary: ResponsesStreamEvent = serde_json::from_str(
        r#"{"type":"response.reasoning_summary_text.delta","item_id":"rs_1","output_index":0,"summary_index":0,"delta":"because"}"#,
    )
    .unwrap();
    assert!(matches!(
        summary,
        ResponsesStreamEvent::ReasoningSummaryTextDelta { delta, .. } if delta == "because"
    ));
}
