# async-llm protocol consolidation design

**Date:** 2026-08-07

## Goal

Move Horsie's OpenAI Chat Completions provider, OpenAI Responses provider, and deterministic mock LLM server into `async-llm`. Publish one feature-gated `async-llm` 0.9.0 release, then make Horsie consume that published release instead of owning those three crates.

The existing Anthropic Messages API remains compatible for current `async-llm` users.

## Package structure

`async-llm` remains one package rather than becoming a family of separately versioned crates. It is reorganized into protocol modules enabled by Cargo features:

| Module | Feature | Responsibility |
| --- | --- | --- |
| `async_llm::anthropic` | `anthropic` | Existing Anthropic Messages API client |
| `async_llm::openai` | `openai` | OpenAI-compatible Chat Completions client and native wire types |
| `async_llm::responses` | `responses` | OpenAI Responses API client, native wire types, and ChatGPT credential support |
| `async_llm::mock` | `mock` | Deterministic mock server implementing all three protocol surfaces |

The public feature configuration is:

```toml
[features]
default = ["anthropic", "rustls"]
anthropic = []
openai = []
responses = ["openai"]
mock = ["anthropic", "openai", "responses"]
rustls = ["reqwest/rustls-tls-native-roots"]
native-tls = ["reqwest/native-tls"]
```

The implementation also makes mock-server-only dependencies optional and enables them exclusively through `mock`. Protocol-only users do not compile those dependencies, and users enable only the APIs they need. The standalone mock binary declares `required-features = ["mock"]`.

The root `async_llm::Client` remains a compatibility re-export of `async_llm::anthropic::Client` when the `anthropic` feature is enabled. New APIs are imported from their explicit modules, avoiding collisions between protocol-specific `Client`, `Message`, and `Usage` names.

## Public API boundary

Each protocol module exposes its own native, Serde-serializable protocol types. These are not Fluorite types and there is no cross-provider normalized request or event model.

### Chat Completions

`async_llm::openai` provides typed Chat Completions requests, messages, tools, stream options, response chunks/deltas, usage, and status errors. It preserves the current behavior for OpenAI-compatible servers:

- streaming request execution;
- fragmented tool-call accumulation;
- reasoning traces under `reasoning_content` and `reasoning`;
- cached-token usage fields;
- retry classification for retryable statuses;
- an incomplete stream is an error unless a protocol terminal frame was received; and
- a retry is allowed only before output has been emitted.

### Responses

`async_llm::responses` provides native Responses input/output item and SSE event types. It retains API-key and ChatGPT OAuth credentials, access-token refresh, encrypted reasoning content, function-call streaming, incomplete-response handling, usage parsing, and the same retry/terminal-frame safeguards.

### Mock server

`async_llm::mock` retains the existing Rust and control-plane behavior:

- `MockLlmServer`, its builder, `MockResponse`, `Scenario`, and queue/capture/reset APIs;
- the standalone server binary;
- Anthropic Messages at `/v1/messages`;
- Chat Completions at `/v1/chat/completions`; and
- Responses at `/responses` and `/v1/responses`.

It depends only on `async-llm` protocol modules plus its optional server/test dependencies. It has no dependency on Horsie.

## Horsie integration

Horsie retains narrow local adapters that implement `horsie_agentcore::LlmProvider`. An adapter:

1. maps `CompletionRequest` into the native request types for the selected protocol;
2. forwards streamed native protocol events as Horsie `AgentEvent`s;
3. maps completed native responses to Horsie `ContentPart`, `StopReason`, and `Usage`; and
4. maps native status/network/protocol errors to Horsie's existing `LlmError` classifications.

This boundary deliberately keeps Fluorite and Horsie domain types out of `async-llm`, while preserving Horsie's configuration, model cards, environment variables, event behavior, and error semantics.

After migration, Horsie removes the `providers/openai`, `providers/openai-responses`, and `providers/mock-llm` workspace members and source trees. Its tests and binaries use `async_llm::mock`.

## Verification and release

The migration carries the existing protocol-wire tests, provider behavior tests, mock tests, and Horsie conformance/end-to-end tests to their new ownership boundaries. Add feature-matrix coverage for:

- default Anthropic compatibility;
- `openai`;
- `responses`;
- `mock`; and
- the complete all-protocol feature set.

Release order is strict:

1. Merge and verify the `async-llm` change.
2. Update the changelog and package version to `0.9.0`.
3. Publish `async-llm` through the existing tag-triggered trusted-publishing workflow.
4. Verify the published crate can be resolved by a clean consumer.
5. Update Horsie to the released `async-llm = "0.9.0"` dependency with the required features.
6. Verify and merge the Horsie migration after CI passes.

This ordering ensures Horsie's lockfile resolves an immutable published package rather than a path dependency or unreleased version.
