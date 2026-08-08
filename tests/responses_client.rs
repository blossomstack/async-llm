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
    use async_llm::responses::chatgpt::{ChatGptAuth, ChatGptTokens, StoredTokens, TokenStore};
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use wiremock::{
        matchers::{method, path},
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

    let id_token = "eyJhbGciOiJub25lIn0.eyAiaHR0cHM6Ly9hcGkub3BlbmFpLmNvbS9hdXRoIjp7ImNoYXRncHRfYWNjb3VudF9pZCI6ImFjY3RfcHJlZmVycmVkIn0sImFjY291bnRfaWQiOiJhY2N0X2ZhbGxiYWNrIn0.";
    let stored = StoredTokens::new("old-access", "refresh", id_token).unwrap();
    assert_eq!(stored.account_id.as_deref(), Some("acct_preferred"));

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(wiremock::matchers::body_string(
            "grant_type=refresh_token&refresh_token=refresh&client_id=test-client",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "new-access",
            "refresh_token": "new-refresh",
            "id_token": id_token
        })))
        .mount(&server)
        .await;

    let store = Arc::new(RecordingStore::default());
    let tokens = ChatGptTokens::new(
        stored,
        store.clone(),
        ChatGptAuth::new("test-client").with_issuer(server.uri()),
    );
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

/// The device-auth endpoints as OpenAI actually answers them: `interval` is a
/// string, there is no `verification_uri`, and every call carries `originator`.
#[tokio::test]
async fn device_login_start_and_poll_use_native_auth_payloads() {
    use async_llm::responses::chatgpt::{
        poll_device_login, start_device_login, ChatGptAuth, StoredTokens, TokenStore,
    };
    use async_trait::async_trait;
    use std::sync::Arc;
    use wiremock::{
        matchers::{body_json, header, method, path},
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
        .and(header("originator", "test-tool"))
        .and(body_json(serde_json::json!({"client_id": "test-client"})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "device_auth_id": "device_1",
            "user_code": "ABCD",
            "interval": "7"
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/token"))
        .and(header("originator", "test-tool"))
        .and(body_json(
            serde_json::json!({"device_auth_id": "device_1", "user_code": "ABCD"}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "authorization_code": "approved_code",
            "code_verifier": "approved_verifier"
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(header("originator", "test-tool"))
        .and(wiremock::matchers::body_string_contains(
            "redirect_uri=http",
        ))
        .and(wiremock::matchers::body_string_contains(
            "deviceauth%2Fcallback",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "access",
            "refresh_token": "refresh"
        })))
        .mount(&server)
        .await;

    let auth = ChatGptAuth::new("test-client")
        .with_issuer(server.uri())
        .with_originator("test-tool");
    let http_client = reqwest::Client::new();
    let login = start_device_login(&http_client, &auth).await.unwrap();
    assert_eq!(login.interval, 7);
    assert_eq!(login.verification_uri, auth.verification_url());
    assert!(login.verification_uri.ends_with("/codex/device"));

    let poll = poll_device_login(&http_client, &auth, &login, Arc::new(EmptyStore))
        .await
        .unwrap();
    let async_llm::responses::chatgpt::DeviceLoginPoll::Approved(tokens) = poll else {
        panic!("device authorization should be approved");
    };
    assert_eq!(tokens.access_token().await.unwrap(), "access");
}

/// An unapproved code is answered with a 4xx that names no standard OAuth
/// error. Reporting that as a failure would abort a login that is merely
/// unfinished.
#[tokio::test]
async fn an_unapproved_device_code_is_pending_whatever_the_4xx_body_says() {
    use async_llm::responses::chatgpt::{
        poll_device_login, ChatGptAuth, DeviceLogin, DeviceLoginPoll, StoredTokens, TokenStore,
    };
    use async_trait::async_trait;
    use std::sync::Arc;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    struct Store;

    #[async_trait]
    impl TokenStore for Store {
        async fn load(&self) -> Result<Option<StoredTokens>, async_llm::responses::ResponsesError> {
            Ok(None)
        }

        async fn save(&self, _: StoredTokens) -> Result<(), async_llm::responses::ResponsesError> {
            Ok(())
        }
    }

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/token"))
        .respond_with(ResponseTemplate::new(400).set_body_string("not yet"))
        .mount(&server)
        .await;

    let auth = ChatGptAuth::new("client").with_issuer(server.uri());
    let login = DeviceLogin::new("device", "user", "", 1);
    assert!(matches!(
        poll_device_login(&reqwest::Client::new(), &auth, &login, Arc::new(Store))
            .await
            .unwrap(),
        DeviceLoginPoll::Pending
    ));
}

#[tokio::test]
async fn chatgpt_client_uses_codex_endpoint_and_account_header() {
    use async_llm::responses::{
        chatgpt::{ChatGptAuth, ChatGptTokens, StoredTokens, TokenStore},
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

    let id_token = "eyJhbGciOiJub25lIn0.eyAiaHR0cHM6Ly9hcGkub3BlbmFpLmNvbS9hdXRoIjp7ImNoYXRncHRfYWNjb3VudF9pZCI6ImFjY3RfMSJ9fQ.";
    let tokens = Arc::new(ChatGptTokens::new(
        StoredTokens::new("access", "refresh", id_token).unwrap(),
        Arc::new(EmptyStore),
        ChatGptAuth::new("test-client").with_issuer("https://auth.openai.com"),
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

#[test]
fn responses_request_includes_encrypted_reasoning_and_prompt_cache_key() {
    let mut request = ResponsesRequest::for_text("gpt-5", "hello");
    request.prompt_cache_key = Some("tenant:42".into());
    let json = serde_json::to_value(request).unwrap();

    assert_eq!(
        json["include"],
        serde_json::json!(["reasoning.encrypted_content"])
    );
    assert_eq!(json["prompt_cache_key"], "tenant:42");
}

#[test]
fn reasoning_output_and_text_delta_preserve_encrypted_content_and_summary() {
    let output: ResponsesStreamEvent = serde_json::from_str(
        r#"{"type":"response.output_item.done","output_index":0,"item":{"type":"reasoning","encrypted_content":"opaque","summary":[{"type":"summary_text","text":"why"}]}}"#,
    )
    .unwrap();
    assert!(matches!(
        output,
        ResponsesStreamEvent::OutputItemDone { item, .. }
            if item.encrypted_content.as_deref() == Some("opaque")
                && item.summary[0].text.as_deref() == Some("why")
    ));

    let delta: ResponsesStreamEvent = serde_json::from_str(
        r#"{"type":"response.reasoning_text.delta","item_id":"rs_1","output_index":0,"content_index":0,"delta":"because"}"#,
    )
    .unwrap();
    assert!(matches!(
        delta,
        ResponsesStreamEvent::ReasoningTextDelta { delta, .. } if delta == "because"
    ));
}

#[test]
fn id_token_account_claims_prefer_top_level_then_namespaced_then_organization() {
    use async_llm::responses::chatgpt::StoredTokens;

    let top_level = "eyJhbGciOiJub25lIn0.eyAiY2hhdGdwdF9hY2NvdW50X2lkIjoidG9wIiwiaHR0cHM6Ly9hcGkub3BlbmFpLmNvbS9hdXRoIjp7ImNoYXRncHRfYWNjb3VudF9pZCI6Im5hbWVzcGFjZWQifSwib3JnYW5pemF0aW9ucyI6W3siaWQiOiJvcmcifV19.";
    let namespaced = "eyJhbGciOiJub25lIn0.eyAiaHR0cHM6Ly9hcGkub3BlbmFpLmNvbS9hdXRoIjp7ImNoYXRncHRfYWNjb3VudF9pZCI6Im5hbWVzcGFjZWQifSwib3JnYW5pemF0aW9ucyI6W3siaWQiOiJvcmcifV19.";
    let organization = "eyJhbGciOiJub25lIn0.eyJvcmdhbml6YXRpb25zIjpbeyJpZCI6Im9yZyJ9XX0.";

    assert_eq!(
        StoredTokens::new("a", "r", top_level)
            .unwrap()
            .account_id
            .as_deref(),
        Some("top")
    );
    assert_eq!(
        StoredTokens::new("a", "r", namespaced)
            .unwrap()
            .account_id
            .as_deref(),
        Some("namespaced")
    );
    assert_eq!(
        StoredTokens::new("a", "r", organization)
            .unwrap()
            .account_id
            .as_deref(),
        Some("org")
    );
}

#[tokio::test]
async fn refreshes_expiring_tokens_with_form_data_and_retains_missing_claims() {
    use async_llm::responses::chatgpt::{ChatGptAuth, ChatGptTokens, StoredTokens, TokenStore};
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
            "grant_type=refresh_token&refresh_token=old-refresh&client_id=client",
        ))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({"access_token":"new-access","expires_in":3600})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let store = Arc::new(Store::default());
    let tokens = ChatGptTokens::new(
        StoredTokens {
            access_token: "old-access".into(),
            refresh_token: "old-refresh".into(),
            id_token: Some("kept-id".into()),
            account_id: Some("kept-account".into()),
            expires_at: Some(0),
        },
        store.clone(),
        ChatGptAuth::new("client").with_issuer(server.uri()),
    );

    assert_eq!(tokens.access_token().await.unwrap(), "new-access");
    let persisted = store.0.lock().unwrap()[0].clone();
    assert_eq!(persisted.refresh_token, "old-refresh");
    assert_eq!(persisted.id_token.as_deref(), Some("kept-id"));
    assert_eq!(persisted.account_id.as_deref(), Some("kept-account"));
    assert!(persisted.expires_at.is_some());
}

#[tokio::test]
async fn responses_stream_parses_success_without_content_type() {
    use async_llm::responses::Client;
    use tokio_stream::StreamExt;
    use wiremock::{
        matchers::{method, path},
        Mock, MockServer, ResponseTemplate,
    };

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/responses"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("data: {\"type\":\"response.completed\",\"response\":{}}\n\n"),
        )
        .mount(&server)
        .await;
    let events = Client::with_api_key("key")
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

#[tokio::test]
async fn device_login_pending_poll_then_approval_exchanges_authorization_code() {
    use async_llm::responses::chatgpt::{
        poll_device_login, ChatGptAuth, DeviceLogin, DeviceLoginPoll, StoredTokens, TokenStore,
    };
    use async_trait::async_trait;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use wiremock::{
        matchers::{body_string_contains, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    struct Store;
    #[async_trait]
    impl TokenStore for Store {
        async fn load(&self) -> Result<Option<StoredTokens>, async_llm::responses::ResponsesError> {
            Ok(None)
        }
        async fn save(&self, _: StoredTokens) -> Result<(), async_llm::responses::ResponsesError> {
            Ok(())
        }
    }
    let server = MockServer::start().await;
    let polls = Arc::new(AtomicUsize::new(0));
    let poll_responder = {
        let polls = polls.clone();
        move |_: &wiremock::Request| {
            if polls.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(403)
                    .set_body_json(serde_json::json!({"error":"authorization_pending"}))
            } else {
                ResponseTemplate::new(200).set_body_json(
                    serde_json::json!({"authorization_code":"code","code_verifier":"verifier"}),
                )
            }
        }
    };
    Mock::given(method("POST"))
        .and(path("/api/accounts/deviceauth/token"))
        .respond_with(poll_responder)
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string_contains(
            "grant_type=authorization_code&client_id=client&code=code&redirect_uri=",
        ))
        .and(body_string_contains(
            "deviceauth%2Fcallback&code_verifier=verifier",
        ))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                serde_json::json!({"access_token":"access","refresh_token":"refresh"}),
            ),
        )
        .expect(1)
        .mount(&server)
        .await;
    let client = reqwest::Client::new();
    let auth = ChatGptAuth::new("client").with_issuer(server.uri());
    let login = DeviceLogin::new("device", "user", "", 1);
    let store = Arc::new(Store);
    assert!(matches!(
        poll_device_login(&client, &auth, &login, store.clone())
            .await
            .unwrap(),
        DeviceLoginPoll::Pending
    ));
    let DeviceLoginPoll::Approved(tokens) = poll_device_login(&client, &auth, &login, store)
        .await
        .unwrap()
    else {
        panic!("expected approval")
    };
    assert_eq!(tokens.access_token().await.unwrap(), "access");
}

#[tokio::test]
async fn chatgpt_401_refreshes_once_and_retries_before_emitting_output() {
    use async_llm::responses::{
        chatgpt::{ChatGptAuth, ChatGptTokens, StoredTokens, TokenStore},
        Client,
    };
    use async_trait::async_trait;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    use tokio_stream::StreamExt;
    use wiremock::{
        matchers::{body_string, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    struct Store;
    #[async_trait]
    impl TokenStore for Store {
        async fn load(&self) -> Result<Option<StoredTokens>, async_llm::responses::ResponsesError> {
            Ok(None)
        }
        async fn save(&self, _: StoredTokens) -> Result<(), async_llm::responses::ResponsesError> {
            Ok(())
        }
    }
    let server = MockServer::start().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let responder = {
        let calls = calls.clone();
        move |_: &wiremock::Request| {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(401)
            } else {
                ResponseTemplate::new(200).set_body_raw(
                    "data: {\"type\":\"response.completed\",\"response\":{}}\n\n",
                    "text/event-stream",
                )
            }
        }
    };
    Mock::given(method("POST"))
        .and(path("/backend-api/codex/responses"))
        .respond_with(responder)
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/oauth/token"))
        .and(body_string(
            "grant_type=refresh_token&refresh_token=refresh&client_id=client",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"access_token":"refreshed","refresh_token":"refresh"}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    let tokens = Arc::new(ChatGptTokens::new(
        StoredTokens {
            access_token: "expired".into(),
            refresh_token: "refresh".into(),
            id_token: None,
            account_id: None,
            expires_at: None,
        },
        Arc::new(Store),
        ChatGptAuth::new("client").with_issuer(server.uri()),
    ));
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
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
