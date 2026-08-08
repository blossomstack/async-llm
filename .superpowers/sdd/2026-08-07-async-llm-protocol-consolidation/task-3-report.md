# Task 3 report — native OpenAI Responses and ChatGPT credentials

## Status

Implemented Task 3 in the requested isolated `async-llm-protocol-consolidation` worktree and committed it on the branch based at `190912a5e4f8c9235904533691e09d1144f465bf`.

The implementation is native to `async-llm`: it has no Horsie, Fluorite, or `LlmProvider` imports or dependencies. `responses = ["openai"]` preserves the required feature relation. The default Anthropic surface and the Task 2 OpenAI module remain feature-gated and unchanged.

## TDD red evidence

Production code was added only after these observed failing tests:

1. Initial Responses request/event contract:

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   ```

   Failed with exit 101 before `src/responses` existed:

   ```text
   error[E0583]: file not found for module `responses`
   --> src/lib.rs:13:1
   ```

2. Native request controls and streamed text/function/reasoning event variants:

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   ```

   Failed with exit 101 before those APIs were implemented, including unresolved `FunctionTool`/`ReasoningControl`, missing `ResponsesRequest::new`, and missing `OutputTextDelta`, `FunctionCallArgumentsDelta`, and `ReasoningEncryptedContent` variants.

3. API-key client and error surface:

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   ```

   Failed with exit 101 on unresolved `async_llm::responses::{Client, ResponsesError}`.

4. ChatGPT storage/device-flow API:

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   ```

   Failed with exit 101 because `async_llm::responses::chatgpt` did not exist.

5. ChatGPT client credential transport:

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   ```

   Failed with exit 101 because `Credential` was private and `Client::with_chatgpt` was absent.

6. Output-item and reasoning-summary event variants:

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   ```

   Failed with exit 101 because `OutputItemDone` and `ReasoningSummaryTextDelta` did not exist.

Each red phase was followed by the minimum implementation and an observed green run of the same test target.

## Changed files

- `Cargo.toml`
  - Makes `responses` enable `openai`.
  - Adds normal dependencies for the native async `TokenStore` trait and JWT payload decoding.
- `Cargo.lock`
  - Records the direct `base64` dependency.
- `src/responses/types.rs`
  - Native request/tool/reasoning/input/output/usage structures and Responses SSE event deserialization.
- `src/responses/mod.rs`
  - Native API-key and ChatGPT Responses stream client, error/retry classification, endpoint/header selection, terminal-frame checks, and stream type.
- `src/responses/chatgpt.rs`
  - Stored credentials, generic async persistence trait, refresh behavior, device authorization/poll flow, and ID-token account-id extraction.
- `tests/responses_client.rs`
  - Direct `wiremock` and parser coverage for request, stream, device-code, and credential behavior.
- `tests/responses_review.rs`
  - Native terminal/error/final-output/unknown-event parser regression coverage; no Task 4 mock module is added.

## Exact public API

```rust
pub use async_llm::responses::{
    Client, Credential, FunctionTool, ReasoningControl, ResponsesError,
    ResponsesRequest, ResponsesStream, ResponsesStreamEvent,
};

impl Client {
    pub fn with_api_key(key: impl Into<secrecy::SecretString>) -> Self;
    pub fn with_chatgpt(tokens: Arc<chatgpt::ChatGptTokens>) -> Self;
    pub fn with_base_url(self, base_url: impl Into<String>) -> Self;
    pub fn max_retries(self, max_retries: u32) -> Self;
    pub async fn stream(
        &self,
        request: ResponsesRequest,
    ) -> Result<ResponsesStream, ResponsesError>;
}

impl ResponsesRequest {
    pub fn new(model: impl Into<String>, input: Vec<serde_json::Value>) -> Self;
    pub fn for_text(model: impl Into<String>, text: impl Into<String>) -> Self;
    pub fn with_function_tool(self, tool: FunctionTool) -> Self;
    pub fn with_reasoning(self, reasoning: ReasoningControl) -> Self;
}

pub use async_llm::responses::chatgpt::{
    ChatGptTokens, DeviceLogin, DeviceLoginPoll, StoredTokens, TokenStore,
    DEFAULT_ISSUER, poll_device_login, start_device_login,
};

pub async fn start_device_login(
    http_client: &reqwest::Client,
    issuer: impl AsRef<str>,
    client_id: impl Into<String>,
) -> Result<DeviceLogin, ResponsesError>;

pub async fn poll_device_login(
    http_client: &reqwest::Client,
    issuer: impl AsRef<str>,
    client_id: impl Into<String>,
    login: &DeviceLogin,
    store: Arc<dyn TokenStore>,
) -> Result<DeviceLoginPoll, ResponsesError>;
```

`ResponsesRequest` deliberately takes native `Vec<serde_json::Value>` input items; history conversion remains adapter-owned. `ResponsesStreamEvent` exposes text, function-argument, reasoning summary/encrypted-content, output-item, completed, incomplete, failed, and forward-compatible other events. `response.incomplete` remains an event, and `is_max_output_tokens()` preserves the protocol reason.

## Tests and results

All successful commands used stable Rust 1.96.0:

```sh
cargo +1.96.0 test --no-default-features --features responses --test responses_client
# 11 passed

cargo +1.96.0 check --no-default-features
# passed

cargo +1.96.0 check --no-default-features --features responses,rustls
# passed

cargo +1.96.0 test
# passed: 17 existing tests + 1 doctest; feature-gated Responses target compiled as 0 tests

rustfmt +1.96.0 --edition 2021 --check \
  src/responses/mod.rs src/responses/types.rs src/responses/chatgpt.rs \
  tests/responses_client.rs
# passed

git diff --check
# passed
```

The 11 native Responses tests cover request flags/native input/tools/reasoning, incomplete reasons, text/function/encrypted-reasoning/summary/output-item events, API-key request/header/path, ChatGPT Codex endpoint and account header, completed and cut streams, HTTP 429 classification/body capture, device authorization and polling payloads, account-id precedence, and refresh persistence.

## Commits

- `62d0c58233b99eae6ed2c7afdc9bc0572323ef68` — `feat: add OpenAI Responses client`
- `e095fe6` — `fix: complete Responses protocol handling`
- `7782a1f6cea2480d562231654ac26843bdab8154` — `fix: preserve Responses terminal errors`

## Concerns

- The Task 3 brief's mock-backed `responses,mock` test command is deliberately not run: Task 4 owns `async_llm::mock`, and no placeholder/mock implementation was added. Task 3 uses existing direct `wiremock` tests instead.
- `cargo +1.96.0 test --default-features` is not a valid Cargo invocation (default features are selected by omitting feature flags); the successful default verification used `cargo +1.96.0 test`.

## Review fix

### Additional TDD evidence

Each review behavior was covered in `tests/responses_client.rs`; all commands used Rust 1.96.0.

1. Device authorization result modeling and OAuth exchange:

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   # RED (exit 101): unresolved import `DeviceLoginPoll`
   ```

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   # GREEN: 12 passed
   ```

   The local wiremock coverage now observes a 403 `authorization_pending`, a later approved `{authorization_code, code_verifier}`, and exactly one form-encoded `/oauth/token` exchange.

2. Successful SSE without `Content-Type`:

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   # RED (exit 101): `chatgpt_streams_sse_without_a_content_type_header` assertion failed
   ```

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   # GREEN: 13 passed
   ```

3. Reasoning payloads and prompt cache key:

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   # RED (exit 101): missing `prompt_cache_key`, output `encrypted_content`/`summary`, and `ReasoningTextDelta`
   ```

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   # GREEN: 13 passed
   ```

4. Account-claim precedence:

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   # RED (exit 101): top-level claim resolved to the namespaced claim
   ```

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   # GREEN: 18 passed
   ```

5. Final focused verification (including expiry-triggered form refresh with retained fallback values and the one-shot ChatGPT 401 refresh/retry):

   ```sh
   cargo +1.96.0 test --no-default-features --features responses --test responses_client
   # GREEN: 18 passed
   ```

### Review-fix implementation

- `src/responses/chatgpt.rs`
  - Models device polling as `DeviceLoginPoll::{Pending, Approved}` and exchanges approved authorization codes using form encoding.
  - Persists token expiry, proactively refreshes expiring tokens, uses form-encoded OAuth refreshes, and retains prior refresh token, ID token, and account when omitted by a refresh response.
  - Applies account claim precedence: top-level claim, namespaced claim, then first organization ID.
- `src/responses/mod.rs`
  - Reads successful response bodies directly and parses SSE frames without requiring a `Content-Type` header.
  - Retains native HTTP status classification and makes one ChatGPT refresh/retry after an initial 401 before any event is emitted.
- `src/responses/types.rs`
  - Requests `reasoning.encrypted_content` by default, serializes optional `prompt_cache_key`, captures top-level reasoning encrypted content and summary, and recognizes `response.reasoning_text.delta`.
- `Cargo.toml`
  - Enables reqwest's stream support for direct successful-response body parsing.
- `tests/responses_client.rs`
  - Adds direct wiremock coverage for all reviewed protocol and credential cases; no `async_llm::mock` module or mock dependency was added.

### Final review verification

```sh
cargo +1.96.0 check --no-default-features --features responses
cargo +1.96.0 check --no-default-features
rustfmt +1.96.0 --edition 2021 --check \
  src/responses/mod.rs src/responses/types.rs src/responses/chatgpt.rs \
  tests/responses_client.rs
git diff --check
```

All passed.

## Review findings follow-up

### TDD evidence

Each added regression was run under Rust 1.96.0 before its production change:

1. `failed_response_and_error_events_preserve_server_details` initially failed with missing `CompletedResponse::status` / `error` fields and no `ResponsesStreamEvent::Error` variant. After adding the typed failure/error data and marking SSE `error` frames terminal, it passed.
2. `final_refusal_and_unrecognized_events_preserve_protocol_data` initially failed because final text/function argument variants, refusal content, and payload-carrying `Other` were absent. It passed after adding those protocol types and the forward-compatible deserializer.
3. `refresh_response_without_id_or_refresh_tokens_keeps_stored_identity` was run with the token response fallback removed and failed with `Authentication("token response omitted refresh_token")` for the exact `{"access_token":"replacement-access"}` response. Restoring the merge with the previous `StoredTokens` made it pass, including persisted refresh token, ID token, and account identity checks.
4. `stored_tokens_debug_redacts_all_token_secrets` initially failed because derived `Debug` contained `access-secret`; the explicit redacting implementation made it pass.
5. `error_event_is_terminal_without_an_incomplete_stream_error` was run with `Error` excluded from the terminal set and failed because the stream added `IncompleteStream`; restoring terminal handling made it pass. `failed_event_is_terminal_without_an_incomplete_stream_error` verifies the corresponding failed-response behavior.

### Implementation

- `response.failed` now retains response status and typed error details; typed `error` SSE events end streams without an `IncompleteStream` artifact.
- Adds typed final output-text and function-argument events and preserves refusal content. Unknown event names and their entire raw JSON payload are represented by `ResponsesStreamEvent::Other { event_type, data }`.
- OAuth refresh responses merge omitted ID/refresh tokens and account identity from the prior stored values.
- `StoredTokens` has a redacting `Debug` implementation; access, refresh, and ID token values are never formatted.

### Verification

```sh
rustfmt +1.96.0 --edition 2021 --check \
  src/responses/mod.rs src/responses/types.rs src/responses/chatgpt.rs \
  tests/responses_client.rs tests/responses_review.rs
cargo +1.96.0 test --no-default-features --features responses --test responses_client
# 18 passed
cargo +1.96.0 test --no-default-features --features responses --test responses_review
# 6 passed
cargo +1.96.0 check --no-default-features --features responses
git diff --check
```
