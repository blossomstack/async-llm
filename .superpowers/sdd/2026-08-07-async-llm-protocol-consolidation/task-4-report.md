# Task 4 Report: Feature-Gated Three-Protocol Mock Server

## Status

Implemented the native `async_llm::mock` server and its required-feature standalone binary. The mock server now exposes the shared FIFO response queue, scenario/session state, delay/block controls, request capture/reset control plane, and Anthropic Messages, OpenAI Chat Completions, and OpenAI Responses routes without Horsie imports.

## Red/green evidence

1. **RED:** Before the mock module existed, `cargo +1.96.0 test --no-default-features --features mock --test mock_server` failed with `E0583`, reporting that `src/mock/mod.rs` was missing for the feature-gated `async_llm::mock` module.
2. **GREEN:** After porting the server and adding optional mock dependencies, the same command passed all three mock-server integration tests.
3. **RED:** The native Responses client integration test initially received `response.output_text.delta` as `ResponsesStreamEvent::Other`; the copied wire frame lacked required `content_index`.
4. **GREEN:** Adding `content_index: 0` to mock Responses text-delta frames made the native client parse `OutputTextDelta` and the test pass.

## Moved/changed files

- Added `src/mock/mod.rs`, `src/mock/server.rs`, `src/mock/anthropic.rs`, `src/mock/openai.rs`, and `src/mock/responses.rs`, porting the shared mock contract and routing all three inference protocols through one FIFO queue.
- Added `src/bin/async-llm-mock.rs`, a required-`mock` standalone server binary with `--port`, `--bind-all`, `PORT`, and help handling.
- Updated `Cargo.toml` and `Cargo.lock` with optional Axum, futures, parking-lot, and UUID dependencies; `mock` activates the protocol features and needed Tokio runtime/network/synchronization features.
- Added `tests/mock_server.rs` to cover direct Anthropic/OpenAI/Responses route behavior and native OpenAI and Responses client consumption against the real mock server.

## Tests

- `cargo +1.96.0 fmt --all -- --check` — passed.
- `cargo +1.96.0 test --no-default-features --features mock --test mock_server` — passed (3 tests).
- `cargo +1.96.0 check --no-default-features --features mock --bin async-llm-mock` — passed.
- `cargo +1.96.0 test --no-default-features --features mock` — passed (65 tests plus 1 doc test).
- `cargo +1.96.0 check --no-default-features` — passed.
- `cargo +1.96.0 run --quiet --no-default-features --features mock --bin async-llm-mock -- --help` — passed.
- `cargo +1.96.0 check --no-default-features --bin async-llm-mock` — intentionally failed because the binary correctly requires `mock`.

## Commits

- `feat: add feature-gated LLM mock server`

## Concern

None. The mock feature intentionally enables the three native protocol features so its mock-backed OpenAI and Responses integration coverage can compile under the required `--features mock` command.
