> [!NOTE]
> Originally the client was forked from [`anthropic-sdk`](https://github.com/Mixpeal/anthropic-sdk) which no longer seems to be maintained. There might still be some references, even though the code has been rewritten from scratch.

## async-llm

A fork of [`async-anthropic`](https://github.com/bosun-ai/async-anthropic), published as `async-llm`, adding thinking-block support, prompt caching (`cache_control` + cache usage fields), and assorted fixes.

A client for the anthropic messages api, written in Rust. There are plenty of clients on crates.io, but we figured we needed another one. Specifically, a straightforward builder api, robust error handling, and room to grow. Tests are also nice.

### Features

- [x] Messages API
- [x] Models API
- [x] Tool use
- [x] Support all API parameters
- [x] Automatic [backoff](https://crates.io/crates/backoff)
- [x] Tracing
- [x] Streaming
- [ ] Non-text messages

### Installation

The default feature set provides the Anthropic-compatible client:

```toml
[dependencies]
async-llm = "0.9"
```

Enable OpenAI Chat Completions with its protocol-native API:

```toml
[dependencies]
async-llm = { version = "0.9", features = ["openai"] }
```

Enable the OpenAI Responses API with its protocol-native request, response, and streaming types:

```toml
[dependencies]
async-llm = { version = "0.9", features = ["responses"] }
```

Enable the deterministic local mock server for tests:

```toml
[dev-dependencies]
async-llm = { version = "0.9", features = ["mock"] }
```

### Native APIs

The OpenAI features expose their protocol-native types rather than translating through the Anthropic API:

```rust
#[cfg(feature = "openai")]
use async_llm::openai::Client as OpenAiClient;

#[cfg(feature = "responses")]
use async_llm::responses::Client as ResponsesClient;

#[cfg(feature = "mock")]
use async_llm::mock::MockLlmServer;
```

`MockLlmServer` serves Anthropic Messages, OpenAI Chat Completions, and OpenAI Responses routes from a deterministic response queue. It is intended for repeatable tests, not production inference.

### Usage

#### Basic Usage

For non-streaming responses, you can use the SDK as follows:

```rust
    let client = Client::default();

    let request = CreateMessagesRequestBuilder::default()
        .model("claude-3-5-sonnet-20241022")
        .messages(vec![MessageBuilder::default()
            .role(MessageRole::User)
            .content("Hello claude!!")
            .build()
            .unwrap()])
        .build()
        .unwrap();

    let response = client.messages().create(request).await?;

    println!("{:?}", response);
```

See `/examples` for more examples.

### Contributing

Contributions are welcome! This project was quickly drafted together to add anthropic support to other bosun projects, and several features are missing. If you'd like to contribute, please open an issue or a pull request.
