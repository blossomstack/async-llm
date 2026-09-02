# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.11.1](https://github.com/blossomstack/async-llm/compare/v0.11.0...v0.11.1) - 2026-09-02

### Added

- `ResponsesUsage::input_tokens_details`, carrying the Responses API's `cached_tokens`. The field was not deserialized, so the cached share of a prompt was dropped at the wire boundary and no consumer could see it — a caller sending a `prompt_cache_key` and getting hits had no way to tell.

  It is an `Option`: a provider that reports no cache accounting is not a provider reporting zero cache hits, and collapsing the two makes an unmeasured cache indistinguishable from a broken one. `cached_tokens` is a subset of `input_tokens`, not an addition to it.

## [0.11.0](https://github.com/blossomstack/async-llm/compare/v0.10.0...v0.11.0) - 2026-08-29

### Added

- Anthropic `image` and `document` content blocks, with a `MediaSource` that is either inline base64 or a URL. Images and documents were the one thing `MessageContent` never carried; the type had carried a `// TODO` for them since it was written.
- `ToolResultBlock`, so an Anthropic `tool_result` can carry an image alongside its text. A tool that produces a screenshot no longer needs a separate message to deliver it.
- OpenAI Chat Completions `image_url` and `file` content parts, via `ChatContentPart`. Note that a `tool`-role message may still carry only text — the API rejects an image inside a tool result, so a caller has to put it in a following user message.

### Changed

- **Breaking.** Anthropic `ToolResult::content` is now `Option<ToolResultContent>` rather than `Option<String>`, and OpenAI `ChatMessage::content` is `Option<ChatContent>` rather than `Option<String>`. Both are untagged, so plain text still crosses the wire as a bare string and the JSON of an existing request is unchanged. `From<String>`/`From<&str>` are implemented for both, and the `ChatMessage` constructors now take `impl Into<ChatContent>`, so `ChatMessage::user("hi")` still compiles.

## [0.10.0](https://github.com/blossomstack/async-llm/compare/v0.9.0...v0.10.0) - 2026-08-07

### Fixed

ChatGPT device login in 0.9.0 could not complete against OpenAI. Every item below is what the endpoints actually do:

- `DeviceLogin::interval` arrives as a JSON string, so it is now read leniently and floors at one second. A numeric-only reader failed the whole user-code response.
- OpenAI's user-code response carries no `verification_uri`; `start_device_login` now derives it from the issuer instead of leaving it empty.
- The `originator` header is sent on the user-code, device-token, token-exchange, refresh, and Codex Responses requests.
- The authorization-code exchange sends `redirect_uri`.
- An unapproved code is reported as `DeviceLoginPoll::Pending` for any unsuccessful poll, not only for a body naming `authorization_pending`.
- A token response without `expires_in` now expires in an hour rather than never, so the credential still refreshes.
- A 5xx from the auth endpoint is `ResponsesError::Overloaded` rather than `Api`, keeping "the endpoint is unwell" apart from "the credential was rejected".

### Added

- `responses::Client::retry_delay`, `connect_timeout`, and `read_timeout`, matching what `openai::ClientBuilder` already exposed. The Responses client's backoff and stall timeout were previously fixed.
- `mock::run_cli`, the `async-llm-mock` binary's argument handling as a library function, so a harness in another workspace can expose the same command without restating it.

### Changed

- **Breaking:** `ChatGptAuth` carries the issuer, client id, and originator. `ChatGptTokens::with_store_and_issuer` becomes `ChatGptTokens::new(tokens, store, auth)`, and `start_device_login`/`poll_device_login` take `&ChatGptAuth` in place of separate issuer and client-id arguments.

## [0.9.0](https://github.com/blossomstack/async-llm/compare/v0.8.0...v0.9.0) - 2026-08-07

### Added

- Feature-gated `openai`, `responses`, and `mock` modules for OpenAI Chat Completions, OpenAI Responses, and deterministic local protocol tests.
- Protocol-native OpenAI Chat Completions and Responses request, response, and streaming types, available through the `openai` and `responses` features.
- The `async-llm-mock` binary, available only with the `mock` feature.

### Changed

- The default feature set continues to provide the existing Anthropic-compatible client API.

## [0.8.0](https://github.com/blossomstack/async-llm/compare/v0.7.0...v0.8.0) - 2026-08-02

### Changed

- **Breaking:** `Thinking::signature` is now `Option<String>` and is omitted from requests when absent. Anthropic-compatible endpoints generally don't supply a replay signature, and an empty string is not a valid substitute.

### Added

- `CreateMessagesRequest::output_config` (`OutputConfig { effort }`) to set reasoning depth on models that support it.

## [0.7.0] - 2026-06-12

First release of the fork under the `async-llm` name.

### Added

- Thinking block support.
- Prompt caching: `cache_control` on content blocks, cache fields on `Usage`.

## [0.6.0](https://github.com/bosun-ai/async-anthropic/compare/v0.5.0...v0.6.0) - 2025-05-03

### Added

- Track input tokens when streaming

## [0.5.0](https://github.com/bosun-ai/async-anthropic/compare/v0.4.0...v0.5.0) - 2025-05-02

### Added

- Support streaming messages with tool use ([#10](https://github.com/bosun-ai/async-anthropic/pull/10))

### Other

- *(deps)* Bump reqwest from 0.12.9 to 0.12.15 in the minor group ([#9](https://github.com/bosun-ai/async-anthropic/pull/9))

## [0.4.0](https://github.com/bosun-ai/async-anthropic/compare/v0.3.0...v0.4.0) - 2025-04-27

### Added

- Add support for models api
- Convenience helpers for accessing message content
- Implement streaming for messages api

### Other

- *(deps)* Bump the minor group with 7 updates ([#8](https://github.com/bosun-ai/async-anthropic/pull/8))
- *(ci)* Add dependabot.yml

## [0.3.0](https://github.com/bosun-ai/async-anthropic/compare/v0.2.1...v0.3.0) - 2025-02-18

### Added

- Add backoff implementation (#5)

### Other

- Add backoff as a major feature

## [0.2.1](https://github.com/bosun-ai/async-anthropic/compare/v0.2.0...v0.2.1) - 2025-02-10

### Added

- Inner content of message list must be public
- Also implement deref mut for message content list

## [0.2.0](https://github.com/bosun-ai/async-anthropic/compare/v0.1.0...v0.2.0) - 2025-02-10

### Added

- Ensure all message content is accessible
- Add convenience method to access first matching text content in message
- Add convenience str to message conversion and partialeq

### Other

- release v0.1.0 (#2)

## [0.1.0](https://github.com/bosun-ai/async-anthropic/releases/tag/v0.1.0) - 2025-02-09

### Added

- Client rewrite

### Fixed

- Add tests

