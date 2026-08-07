# async-llm protocol consolidation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Consolidate Horsie's OpenAI Chat Completions, OpenAI Responses, and deterministic mock-server implementations into feature-gated `async-llm` 0.9.0, then migrate Horsie to that published package.

**Architecture:** Keep one `async-llm` package, partitioned into `anthropic`, `openai`, `responses`, and `mock` modules behind Cargo features. Its OpenAI modules expose native protocol models and streaming clients, never Horsie or Fluorite types. Horsie owns one thin adapter crate which implements its `LlmProvider` contract and maps its events/types to and from the native clients.

**Tech Stack:** Rust 2021/2024, Cargo features, reqwest, reqwest-eventsource, eventsource-stream, Tokio, Axum, Serde, `async-trait`, OpenAI Chat Completions and Responses SSE protocols, crates.io trusted publishing.

## Global Constraints

- Release exactly one `async-llm` package at version `0.9.0`; do not rename it or publish protocol-specific packages.
- Preserve the default `async-llm` Anthropic API: `async_llm::Client`, `types`, `messages`, `models`, and `errors` remain available with `default = ["anthropic", "rustls"]`.
- Gate protocol modules with `anthropic`, `openai`, `responses`, and `mock`; `responses` enables `openai`, and `mock` enables all protocol features plus only its optional server dependencies.
- `async-llm` must not depend on any `horsie-*` crate or Fluorite type.
- Public OpenAI APIs use native protocol-shaped Serde types; do not introduce a provider-neutral cross-protocol request/event abstraction.
- Preserve existing retry classification, native SSE terminal-frame checks, no-retry-after-emission behavior, OAuth refresh, and mock control-plane behavior.
- Horsie must resolve `async-llm = "0.9.0"` from crates.io after publishing; do not retain a path dependency for the release migration.
- Make all Horsie changes in a new worktree under `horsie/.horsie/worktrees`, based on a freshly fetched `origin/main`.
- Use stable Rust 1.96.0 for formatting. Do not invoke nightly rustfmt.

---

## File structure

### async-llm

| File | Responsibility |
| --- | --- |
| `Cargo.toml` | Version, optional dependencies, public protocol/TLS feature graph, feature-gated mock binary |
| `src/lib.rs` | Feature-gated module exports and root Anthropic compatibility re-exports |
| `src/anthropic/*.rs` | Existing client implementation moved without public API breakage |
| `src/openai/mod.rs` | Chat Completions public module, client configuration, stream API, errors |
| `src/openai/types.rs` | Native Chat Completions request/response/SSE types and protocol-only helpers |
| `src/responses/mod.rs` | Responses public module, client configuration, stream API, errors |
| `src/responses/types.rs` | Native Responses request/input/output/SSE types |
| `src/responses/chatgpt.rs` | Device login, persisted token representation, refreshable ChatGPT credential |
| `src/mock/*.rs` | Moved deterministic mock server and the three protocol handlers |
| `src/bin/async-llm-mock.rs` | Standalone mock server binary, gated by `mock` |
| `CHANGELOG.md`, `README.md` | 0.9.0 release notes and feature/module usage |
| `.github/workflows/tests.yml` | Feature-matrix CI coverage |

### Horsie

| File | Responsibility |
| --- | --- |
| `providers/async-llm/Cargo.toml` | Local, unpublished adapter crate depending on released `async-llm` features |
| `providers/async-llm/src/lib.rs` | Adapter module exports |
| `providers/async-llm/src/openai.rs` | `LlmProvider` adapter for Chat Completions |
| `providers/async-llm/src/responses.rs` | `LlmProvider` adapter for Responses |
| `Cargo.toml` | Replace removed members with the adapter crate and retain the Anthropic adapter |
| `server/Cargo.toml` | Depend on adapter crate and `async-llm` Responses feature for OAuth types |
| `server/src/config/store.rs` | Instantiate adapters and use `async_llm::responses::chatgpt` persistence types |
| `server/src/config/chatgpt_login.rs` | Import device-login functions/types from `async-llm` |
| `tests/Cargo.toml` and provider test manifests | Depend on adapter crate and `async-llm` mock feature |
| existing provider/conformance/E2E tests | Update imports without reducing behavioral coverage |

Delete `providers/openai`, `providers/openai-responses`, and `providers/mock-llm` only after all migrated call sites build and test through the released dependency.

## Task 1: Establish the async-llm feature topology and preserve Anthropic compatibility

**Files:**
- Modify: `async-llm/Cargo.toml`
- Modify: `async-llm/src/lib.rs`
- Move: `async-llm/src/{client,errors,messages,models,types}.rs` → `async-llm/src/anthropic/`
- Create: `async-llm/src/anthropic/mod.rs`
- Test: `async-llm/tests/anthropic_compat.rs`

**Consumes:** async-llm 0.8.0 public root `Client`, `errors`, `messages`, `models`, and `types`.

**Produces:** an `anthropic` feature and `async_llm::anthropic::{Client, errors, messages, models, types}` while retaining the current root paths when `anthropic` is enabled.

- [ ] **Step 1: Write the compatibility compile/integration test.**

```rust
use async_llm::{Client, types::CreateMessagesRequestBuilder};

#[test]
fn root_anthropic_exports_remain_available() {
    let _client = Client::default();
    let _builder = CreateMessagesRequestBuilder::default();
}
```

- [ ] **Step 2: Verify the test currently fails because the new test target does not exist.**

Run: `cargo +1.96.0 test --test anthropic_compat`

Expected: Cargo reports no test target named `anthropic_compat`.

- [ ] **Step 3: Move the existing Anthropic source modules under `src/anthropic/` and create the feature-gated root façade.**

```rust
// src/lib.rs
#[cfg(feature = "anthropic")]
pub mod anthropic;
#[cfg(feature = "anthropic")]
pub use anthropic::Client;
#[cfg(feature = "anthropic")]
pub use anthropic::{errors, messages, models, types};

#[cfg(feature = "openai")]
pub mod openai;
#[cfg(feature = "responses")]
pub mod responses;
#[cfg(feature = "mock")]
pub mod mock;
```

Set the package feature defaults exactly to `default = ["anthropic", "rustls"]`; preserve the existing `rustls` and `native-tls` reqwest feature behavior.

- [ ] **Step 4: Make moved Anthropic imports module-relative and run its tests.**

Run: `cargo +1.96.0 test --default-features`

Expected: Existing Anthropic unit tests and `anthropic_compat` pass.

- [ ] **Step 5: Verify the base feature boundary.**

Run: `cargo +1.96.0 check --no-default-features && cargo +1.96.0 check --no-default-features --features anthropic,rustls`

Expected: Both feature selections compile without exposing an unavailable root `Client` in the no-feature build.

- [ ] **Step 6: Commit the feature façade.**

```bash
git add Cargo.toml src/lib.rs src/anthropic tests/anthropic_compat.rs
git commit -m "refactor: feature-gate Anthropic client"
```

## Task 2: Move the native OpenAI Chat Completions client into async-llm

**Files:**
- Create: `async-llm/src/openai/mod.rs`
- Create: `async-llm/src/openai/types.rs`
- Create: `async-llm/tests/openai_client.rs`
- Modify: `async-llm/Cargo.toml`
- Source to port: `horsie/providers/openai/src/{lib.rs,wire.rs}`

**Consumes:** native Chat Completions JSON fields and the existing Horsie provider's retry/SSE rules.

**Produces:** `async_llm::openai::{Client, ClientBuilder, ChatCompletionRequest, ChatCompletionChunk, ChatCompletionError}` with `Client::stream(ChatCompletionRequest) -> impl Stream<Item = Result<ChatCompletionChunk, ChatCompletionError>>`.

- [ ] **Step 1: Write failing native-wire tests without Horsie imports.**

```rust
use async_llm::openai::{ChatCompletionRequest, ChatMessage, StreamOptions};

#[test]
fn serializes_chat_completion_stream_request() {
    let request = ChatCompletionRequest {
        model: "mock-model".into(),
        messages: vec![ChatMessage::user("hello")],
        stream: true,
        stream_options: Some(StreamOptions { include_usage: true }),
        ..Default::default()
    };
    let value = serde_json::to_value(request).unwrap();
    assert_eq!(value["stream"], true);
    assert_eq!(value["stream_options"]["include_usage"], true);
}
```

- [ ] **Step 2: Confirm the test fails before the module exists.**

Run: `cargo +1.96.0 test --no-default-features --features openai --test openai_client`

Expected: unresolved `async_llm::openai` import.

- [ ] **Step 3: Define only native protocol types in `openai/types.rs`.**

Implement public Serde types for request messages, function tools, tool calls, stream options, deltas, choices, usage, and chunks. Keep `Delta::reasoning_trace()` and `WireUsage::cached_tokens()` as protocol helpers. Do not port `to_wire_messages`, `CompletionRequest`, `ContentPart`, or `ThinkingDialect`; those are Horsie adapters' responsibility.

- [ ] **Step 4: Implement the client and streaming error model in `openai/mod.rs`.**

```rust
pub struct Client { /* reqwest client, endpoint, credential, retry/read timeout */ }

impl Client {
    pub fn builder() -> ClientBuilder;
    pub async fn stream(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionStream, ChatCompletionError>;
}

pub type ChatCompletionStream = Pin<Box<dyn Stream<Item = Result<ChatCompletionChunk, ChatCompletionError>> + Send>>;
```

Classify 429 as rate-limited and 500/502/503/504/529 as overloaded. Preserve bearer authentication, bounded connect/idle reads, `[DONE]` recognition, and body capture for non-success statuses. Keep retry policy explicit in the client; the Horsie adapter decides whether a completed native stream can be replayed after it emitted an event.

- [ ] **Step 5: Port the existing wire and mock-backed client tests.**

Use `async_llm::mock::MockLlmServer` in a test built with `--features openai,mock`. Cover text, tool-call fragments, `reasoning_content`, `reasoning`, usage cache tokens, `length`, HTTP 429, and a missing terminal frame.

- [ ] **Step 6: Run the OpenAI feature tests.**

Run: `cargo +1.96.0 test --no-default-features --features openai,mock --test openai_client`

Expected: All native serialization, stream parsing, and error-classification tests pass.

- [ ] **Step 7: Commit the Chat Completions module.**

```bash
git add Cargo.toml src/openai tests/openai_client.rs
git commit -m "feat: add OpenAI Chat Completions client"
```

## Task 3: Move the native OpenAI Responses and ChatGPT credential client into async-llm

**Files:**
- Create: `async-llm/src/responses/mod.rs`
- Create: `async-llm/src/responses/types.rs`
- Create: `async-llm/src/responses/chatgpt.rs`
- Create: `async-llm/tests/responses_client.rs`
- Modify: `async-llm/Cargo.toml`
- Source to port: `horsie/providers/openai-responses/src/{lib.rs,wire.rs,chatgpt.rs}`

**Consumes:** Responses API JSON/SSE shapes and the existing device-code token-refresh behavior.

**Produces:** `async_llm::responses::{Client, ResponsesRequest, ResponsesStreamEvent, ResponsesError}` and `async_llm::responses::chatgpt::{StoredTokens, TokenStore, ChatGptTokens, DeviceLogin, start_device_login, poll_device_login}`.

- [ ] **Step 1: Write failing native request/event tests.**

```rust
use async_llm::responses::{ResponsesRequest, ResponsesStreamEvent};

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
    ).unwrap();
    assert!(event.is_max_output_tokens());
}
```

- [ ] **Step 2: Verify those tests fail before the Responses module exists.**

Run: `cargo +1.96.0 test --no-default-features --features responses --test responses_client`

Expected: unresolved `async_llm::responses` import.

- [ ] **Step 3: Separate native Responses types from Horsie-history conversion.**

Port `FunctionTool`, `ReasoningControl`, request fields, input/output item structures, and SSE event deserialization into `responses/types.rs`. Replace the Horsie-specific `to_input_items(&[horsie_models::agent::Message])` with public constructors that accept already-native `Vec<serde_json::Value>` input items. The Horsie adapter performs history conversion.

- [ ] **Step 4: Implement native client credentials and streaming.**

```rust
pub enum Credential {
    ApiKey(secrecy::SecretString),
    ChatGpt(Arc<chatgpt::ChatGptTokens>),
}

pub struct Client { /* endpoint, model defaults, credential, retry/read timeout */ }

impl Client {
    pub fn with_api_key(key: impl Into<secrecy::SecretString>) -> Self;
    pub fn with_chatgpt(tokens: Arc<chatgpt::ChatGptTokens>) -> Self;
    pub async fn stream(&self, request: ResponsesRequest)
        -> Result<ResponsesStream, ResponsesError>;
}
```

Preserve the API-key endpoint, ChatGPT endpoint/header selection, refresh persistence through `TokenStore`, `encrypted_content`, function-call deltas, incomplete status mapping, and no-terminal-frame error behavior. Move `LlmError` variants to a native `ResponsesError` that carries status/message/retry class.

- [ ] **Step 5: Port device-login tests and mock-backed stream tests.**

Cover device authorization serialization, id-token account-id extraction precedence, refresh persistence, output text events, function calls, reasoning encrypted content, incomplete completion, HTTP status classification, and a cut stream. Tests must use `async_llm::mock` rather than Horsie mock imports.

- [ ] **Step 6: Run the Responses feature tests.**

Run: `cargo +1.96.0 test --no-default-features --features responses,mock --test responses_client`

Expected: all native wire, credential, and stream tests pass.

- [ ] **Step 7: Commit the Responses module.**

```bash
git add Cargo.toml src/responses tests/responses_client.rs
git commit -m "feat: add OpenAI Responses client"
```

## Task 4: Move the deterministic three-protocol mock server behind the mock feature

**Files:**
- Create: `async-llm/src/mock/mod.rs`
- Create: `async-llm/src/mock/server.rs`
- Create: `async-llm/src/mock/anthropic.rs`
- Create: `async-llm/src/mock/openai.rs`
- Create: `async-llm/src/mock/responses.rs`
- Create: `async-llm/src/bin/async-llm-mock.rs`
- Create: `async-llm/tests/mock_server.rs`
- Modify: `async-llm/Cargo.toml`
- Source to port: `horsie/providers/mock-llm/src/{lib.rs,main.rs,server.rs,openai.rs,responses.rs}`

**Consumes:** all native module wire types and the current mock control-plane API.

**Produces:** `async_llm::mock::{MockLlmServer, MockLlmServerBuilder, MockResponse, Scenario, ScenarioConfig, BlockHandle}` and a feature-gated `async-llm-mock` binary.

- [ ] **Step 1: Write mock feature tests for all protocol routes.**

```rust
use async_llm::mock::MockLlmServer;

#[tokio::test]
async fn one_server_serves_all_protocol_routes() {
    let server = MockLlmServer::builder().build().await;
    server.queue_response("hello");
    let anthropic = reqwest::Client::new()
        .post(format!("{}/v1/messages", server.url()))
        .json(&serde_json::json!({"model":"m","max_tokens":1,"messages":[]}))
        .send().await.unwrap();
    assert!(anthropic.status().is_success());
}
```

Add independent calls for `/v1/chat/completions` and `/responses`; assert their expected terminal frames.

- [ ] **Step 2: Verify the mock test fails until the feature/module is implemented.**

Run: `cargo +1.96.0 test --no-default-features --features mock --test mock_server`

Expected: unresolved `async_llm::mock` import.

- [ ] **Step 3: Port the mock server without protocol behavior changes.**

Move its response queue, scenario session bindings, delay/block control, request capture, reset behavior, and each existing handler. Keep the public builder and `MockResponse` variant names unchanged so Horsie's test migration is import-only. Change internal Anthropic imports to `crate::anthropic` types as needed; no Horsie type may appear in any moved file.

- [ ] **Step 4: Gate Axum and mock-only dependencies.**

Mark Axum, parking-lot, UUID, and mock-only futures dependencies optional where they are not already needed by a selected protocol. Ensure the `mock` feature enables exactly the dependencies used by `src/mock` and its binary, and add `required-features = ["mock"]` for `async-llm-mock`.

- [ ] **Step 5: Run server/library and binary checks.**

Run: `cargo +1.96.0 test --no-default-features --features mock --test mock_server && cargo +1.96.0 check --no-default-features --features mock --bin async-llm-mock`

Expected: all three routes work and the binary compiles only when `mock` is selected.

- [ ] **Step 6: Commit the mock module.**

```bash
git add Cargo.toml src/mock src/bin tests/mock_server.rs
git commit -m "feat: add feature-gated LLM mock server"
```

## Task 5: Finish async-llm feature matrix, documentation, and 0.9.0 release preparation

**Files:**
- Modify: `async-llm/Cargo.toml`
- Modify: `async-llm/CHANGELOG.md`
- Modify: `async-llm/README.md`
- Modify: `async-llm/.github/workflows/tests.yml`
- Test: `async-llm/tests/feature_matrix.rs` if a smoke target is needed

**Consumes:** the four feature-gated modules.

**Produces:** a documented, CI-verified 0.9.0 package ready to publish.

- [ ] **Step 1: Add release documentation tests/checks.**

Add one compile smoke target per command below and document imports matching them:

```rust
#[cfg(feature = "openai")]
use async_llm::openai::Client as OpenAiClient;
#[cfg(feature = "responses")]
use async_llm::responses::Client as ResponsesClient;
#[cfg(feature = "mock")]
use async_llm::mock::MockLlmServer;
```

- [ ] **Step 2: Run the test commands before CI changes.**

Run: `cargo +1.96.0 test --default-features && cargo +1.96.0 test --no-default-features --features openai,mock && cargo +1.96.0 test --no-default-features --features responses,mock && cargo +1.96.0 test --all-features`

Expected: all commands pass locally; record and fix any missing optional dependency wiring.

- [ ] **Step 3: Set the package version and changelog entry.**

Set `package.version = "0.9.0"`. Add a `0.9.0` changelog section that lists the `openai`, `responses`, and `mock` modules, their feature names, preserved default Anthropic compatibility, and the feature-gated binary.

- [ ] **Step 4: Document feature selection and native APIs.**

Add README installation examples for default Anthropic, `features = ["openai"]`, `features = ["responses"]`, and `features = ["mock"]`. State that OpenAI modules expose protocol-native types and that mock is intended for deterministic tests.

- [ ] **Step 5: Make CI execute the feature matrix.**

Add distinct CI commands for default, `openai,mock`, `responses,mock`, and `--all-features`; keep stable toolchain 1.96.0 and existing formatting/clippy steps.

- [ ] **Step 6: Verify package contents and quality.**

Run: `cargo +1.96.0 fmt --all -- --check && cargo +1.96.0 clippy --all-targets --all-features -- -D warnings && cargo +1.96.0 test --all-features && cargo +1.96.0 package --allow-dirty`

Expected: formatter, clippy, all features, and package verification pass.

- [ ] **Step 7: Commit release preparation.**

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md README.md .github/workflows/tests.yml tests
git commit -m "release: prepare async-llm 0.9.0"
```

## Task 6: Create Horsie's local async-llm provider adapters

**Files:**
- Create: `horsie/providers/async-llm/Cargo.toml`
- Create: `horsie/providers/async-llm/src/lib.rs`
- Create: `horsie/providers/async-llm/src/openai.rs`
- Create: `horsie/providers/async-llm/src/responses.rs`
- Test: `horsie/providers/async-llm/src/openai.rs`
- Test: `horsie/providers/async-llm/src/responses.rs`

**Consumes:** published `async-llm` `openai`, `responses`, and `mock` modules plus `horsie-agentcore`/`horsie-models`.

**Produces:** `horsie_async_llm::OpenAiProvider` and `horsie_async_llm::ResponsesProvider`, each implementing `LlmProvider` while retaining the constructors/configuration used by `server/src/config/store.rs`.

- [ ] **Step 1: Write failing adapter tests for native-to-Horsie event conversion.**

```rust
#[tokio::test]
async fn openai_adapter_emits_reasoning_then_text() {
    let server = async_llm::mock::MockLlmServer::builder().build().await;
    server.queue_reasoning("reason", "answer");
    let response = provider(server.url()).complete(request(), "msg-1", &sink()).await.unwrap();
    assert!(matches!(response.parts[0], ContentPart::Thinking(_)));
    assert!(matches!(response.parts[1], ContentPart::Text(_)));
}
```

For Responses, add a test asserting encrypted reasoning becomes a Horsie thinking signature and a function-call event becomes `ContentPart::ToolCall`.

- [ ] **Step 2: Run the adapter tests to confirm the crate is absent.**

Run: `cargo +1.96.0 test -p horsie-async-llm --lib`

Expected: Cargo reports that package `horsie-async-llm` does not exist.

- [ ] **Step 3: Add the unpublished adapter crate and its exact dependencies.**

```toml
[dependencies]
horsie-agentcore = { path = "../../agentcore" }
horsie-models = { path = "../../models" }
async-llm = { version = "0.9.0", features = ["openai", "responses"] }
async-trait = { workspace = true }
tokio = { workspace = true, features = ["time"] }
serde_json = { workspace = true }

[dev-dependencies]
async-llm = { version = "0.9.0", features = ["mock"] }
```

Keep it `publish = false` and inherit workspace lints.

- [ ] **Step 4: Implement Chat Completions mapping in `openai.rs`.**

Retain `DEFAULT_MODEL`, `DEFAULT_MAX_TOKENS`, `DEFAULT_BASE_URL`, `env_base_url`, builders (`with_api_key`, `with_model`, `with_base_url`, `with_max_tokens`, `with_thinking_dialect`, `with_forced_tools_disable_thinking`, retry/read timeout), and `LlmProvider::complete`. Move these conversions here:

```rust
fn request_from_completion(request: &CompletionRequest<'_>, model: &str, config: &OpenAiConfig)
    -> async_llm::openai::ChatCompletionRequest;

async fn emit_chunk(
    chunk: async_llm::openai::ChatCompletionChunk,
    state: &mut StreamState,
    message_id: &str,
    events: &dyn EventSink,
) -> Result<Option<StopReason>, LlmError>;
```

Map system text, user/assistant history, tool results, assistant tool calls, thinking exclusion, OpenAI `reasoning_effort`, tool-choice behavior, text/reasoning/tool delta events, cache usage, finish reasons, HTTP classes, and malformed tool JSON exactly as the old provider tests specify.

- [ ] **Step 5: Implement Responses mapping in `responses.rs`.**

Retain provider configuration and `LlmProvider::complete`, but construct native `async_llm::responses::ResponsesRequest` input items from Horsie history. Map output text, reasoning summary/encrypted content, function-call argument deltas, tool calls, terminal/incomplete states, usage, and native errors to existing Horsie events/results. Re-export `async_llm::responses::chatgpt` from this crate only if an existing Horsie caller still requires a compatibility import; otherwise update callers to import it from `async_llm` directly.

- [ ] **Step 6: Run focused adapters tests.**

Run: `cargo +1.96.0 test -p horsie-async-llm --features test-util`

Expected: migrated OpenAI/Responses behavior tests pass against `async_llm::mock`.

- [ ] **Step 7: Commit the Horsie adapters.**

```bash
git add providers/async-llm
git commit -m "feat(providers): adapt async-llm OpenAI clients"
```

## Task 7: Migrate Horsie configuration and every test consumer to the released package

**Files:**
- Modify: `horsie/Cargo.toml`
- Modify: `horsie/server/Cargo.toml`
- Modify: `horsie/server/src/config/store.rs`
- Modify: `horsie/server/src/config/chatgpt_login.rs`
- Modify: `horsie/providers/anthropic/Cargo.toml`
- Modify: `horsie/providers/anthropic/tests/*.rs`
- Modify: `horsie/tests/Cargo.toml`
- Modify: `horsie/tests/tests/{agent_e2e,agent_recovery_e2e,provider_conformance,session_server_e2e}.rs`
- Modify: every remaining import of `horsie_mock_llm`, `horsie_openai`, or `horsie_openai_responses`

**Consumes:** `horsie-async-llm` adapters and release `async-llm` 0.9.0.

**Produces:** Horsie builds/tests only against the published async-llm provider and mock APIs.

- [ ] **Step 1: Update manifest dependencies and make imports fail deliberately.**

Replace `horsie-openai` and `horsie-openai-responses` dependencies with `horsie-async-llm`. Add `async-llm = { version = "0.9.0", features = ["responses"] }` directly to server for ChatGPT device-login/token-store types. Replace each test-only `horsie-mock-llm` dependency with `async-llm = { version = "0.9.0", features = ["mock"] }`.

- [ ] **Step 2: Confirm current imports fail before they are updated.**

Run: `cargo +1.96.0 check --workspace`

Expected: unresolved imports for the removed Horsie provider/mock crates.

- [ ] **Step 3: Update server configuration.**

Use `horsie_async_llm::{OpenAiProvider, ResponsesProvider}` in `store.rs`. Import `ChatGptTokens`, `StoredTokens`, `TokenStore`, `DEFAULT_ISSUER`, `DeviceLogin`, `start_device_login`, and `poll_device_login` directly from `async_llm::responses::chatgpt`. Preserve existing persisted database column mapping and provider construction semantics.

- [ ] **Step 4: Update all test imports without changing assertions.**

```rust
use async_llm::mock::MockLlmServer;
use horsie_async_llm::{OpenAiProvider, ResponsesProvider};
```

Leave existing conformance cases intact: their purpose is to prove the native-client adapters retain behavior. Update the Anthropic provider's mock dev-dependency too, so one mock implementation services every provider test.

- [ ] **Step 5: Run focused Horsie verification.**

Run: `cargo +1.96.0 test -p horsie-async-llm --features test-util && cargo +1.96.0 test -p integration-tests --test provider_conformance && cargo +1.96.0 test -p horsie-server --features test-util config`

Expected: OpenAI/Responses configuration and conformance tests pass through `async-llm`.

- [ ] **Step 6: Commit migrated consumers.**

```bash
git add Cargo.toml server providers/anthropic tests Cargo.lock
git commit -m "refactor: consume async-llm provider modules"
```

## Task 8: Delete extracted Horsie implementations and verify release/PR ordering

**Files:**
- Delete: `horsie/providers/openai/`
- Delete: `horsie/providers/openai-responses/`
- Delete: `horsie/providers/mock-llm/`
- Modify: `horsie/Cargo.toml`
- Modify: `horsie/Cargo.lock`
- Modify: docs/scripts/workflows that name removed packages

**Consumes:** successful migration of all production and test consumers.

**Produces:** no copied provider/mock implementation remains in Horsie and both repositories are ready for separate PRs.

- [ ] **Step 1: Search for prohibited legacy crate names.**

Run: `rg -n 'horsie_(mock_llm|openai_responses|openai)|horsie-mock-llm|horsie-openai-responses|horsie-openai' --glob '!docs/superpowers/{plans,specs}/**' .`

Expected: no production/test source or manifest result; update package references that remain outside historical design documents.

- [ ] **Step 2: Remove old workspace members and source trees.**

Delete the three directories and their workspace member lines only after Step 1 has no active-consumer results. Do not delete `providers/anthropic`.

- [ ] **Step 3: Verify that Horsie resolves the released version, not a path package.**

Run: `cargo +1.96.0 tree -i async-llm && rg -n 'async-llm\s*=\s*\{[^}]*path' Cargo.toml **/Cargo.toml`

Expected: dependency tree identifies `async-llm v0.9.0`; the second command returns no result.

- [ ] **Step 4: Run formatting, clippy, and the required test suites.**

Run: `cargo +1.96.0 fmt --all -- --check && cargo +1.96.0 clippy --all-targets --all-features -- -D warnings && cargo +1.96.0 test --workspace`

Expected: all commands pass. Use `cargo test --workspace`, not a single-crate server test, because Horsie's feature-gated testkit dependencies require workspace feature unification.

- [ ] **Step 5: Commit extraction cleanup.**

```bash
git add -A
git commit -m "refactor: remove migrated LLM providers"
```

- [ ] **Step 6: Deliver in dependency order.**

First push and open the async-llm PR. After its CI passes, merge it, tag `v0.9.0`, and wait for the existing publish workflow to succeed. Verify `cargo search async-llm --limit 1` reports 0.9.0. Then rebase/update the Horsie worktree to the released version, push/open its PR, and wait for its CI to be green and mergeable.

## Plan self-review

- **Spec coverage:** Tasks 1–5 cover the single feature-gated package, default Anthropic compatibility, native protocol APIs, mock behavior, test matrix, version, documentation, and release. Tasks 6–8 cover the Horsie adapter boundary, all production/test consumers, removal of old providers, and published-package ordering.
- **Type consistency:** `async_llm::openai::Client` and `async_llm::responses::Client` are native clients; `horsie_async_llm::OpenAiProvider` and `horsie_async_llm::ResponsesProvider` are the only `LlmProvider` adapters. The mock import is always `async_llm::mock::MockLlmServer`.
- **Scope:** The plan does not move the remaining Horsie Anthropic adapter. It migrates exactly the OpenAI, Responses, and mock implementations requested while preserving Horsie's internal domain boundary.
