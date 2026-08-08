//! OpenAI-compatible Chat Completions streaming client.

mod types;

use async_stream::stream;
use reqwest_eventsource::{retry::Never, Event, EventSource};
use secrecy::{ExposeSecret, SecretString};
use std::{env, pin::Pin, time::Duration};
use thiserror::Error;
use tokio_stream::{Stream, StreamExt as _};

pub use types::{
    ChatCompletionChunk, ChatCompletionRequest, ChatMessage, Choice, Delta, DeltaFunction,
    DeltaToolCall, FunctionCall, FunctionDef, PromptTokensDetails, StreamOptions, ToolCall,
    ToolChoice, ToolDef, WireUsage,
};

const DEFAULT_BASE_URL: &str = "https://api.openai.com";
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_MAX_RETRIES: u32 = 6;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(5);

pub type ChatCompletionStream =
    Pin<Box<dyn Stream<Item = Result<ChatCompletionChunk, ChatCompletionError>> + Send>>;

#[derive(Debug, Error)]
pub enum ChatCompletionError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("rate limited: {body}")]
    RateLimited { body: String },

    #[error("provider overloaded (HTTP {status}): {body}")]
    Overloaded { status: u16, body: String },

    #[error("OpenAI API returned HTTP {status}: {body}")]
    Api { status: u16, body: String },

    #[error("stream ended without a terminal frame")]
    IncompleteStream,

    #[error("stream error: {0}")]
    Stream(String),
}

#[derive(Clone, Debug)]
pub struct Client {
    http_client: reqwest::Client,
    base_url: String,
    api_key: Option<SecretString>,
    max_retries: u32,
    retry_delay: Duration,
}

#[derive(Clone, Debug)]
pub struct ClientBuilder {
    http_client: Option<reqwest::Client>,
    base_url: String,
    api_key: Option<SecretString>,
    connect_timeout: Duration,
    read_timeout: Duration,
    max_retries: u32,
    retry_delay: Duration,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            http_client: None,
            base_url: DEFAULT_BASE_URL.to_string(),
            api_key: env::var("OPENAI_API_KEY")
                .ok()
                .filter(|key| !key.is_empty())
                .map(Into::into),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            read_timeout: DEFAULT_READ_TIMEOUT,
            max_retries: DEFAULT_MAX_RETRIES,
            retry_delay: DEFAULT_RETRY_DELAY,
        }
    }
}

impl ClientBuilder {
    #[must_use]
    pub fn http_client(mut self, http_client: reqwest::Client) -> Self {
        self.http_client = Some(http_client);
        self
    }

    #[must_use]
    pub fn base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }

    #[must_use]
    pub fn api_key(mut self, api_key: impl Into<SecretString>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    #[must_use]
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    #[must_use]
    pub fn read_timeout(mut self, timeout: Duration) -> Self {
        self.read_timeout = timeout;
        self
    }

    #[must_use]
    pub fn max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    #[must_use]
    pub fn retry_delay(mut self, retry_delay: Duration) -> Self {
        self.retry_delay = retry_delay;
        self
    }

    pub fn build(self) -> Result<Client, ChatCompletionError> {
        let http_client = match self.http_client {
            Some(http_client) => http_client,
            None => reqwest::Client::builder()
                .connect_timeout(self.connect_timeout)
                .read_timeout(self.read_timeout)
                .build()?,
        };

        Ok(Client {
            http_client,
            base_url: self.base_url.trim_end_matches('/').to_string(),
            api_key: self.api_key,
            max_retries: self.max_retries,
            retry_delay: self.retry_delay,
        })
    }
}

impl Client {
    #[must_use]
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    pub fn from_api_key(api_key: impl Into<SecretString>) -> Result<Self, ChatCompletionError> {
        Self::builder().api_key(api_key).build()
    }

    fn request(&self, request: &ChatCompletionRequest) -> reqwest::RequestBuilder {
        let mut request_builder = self
            .http_client
            .post(format!("{}/v1/chat/completions", self.base_url))
            .json(request);
        if let Some(api_key) = &self.api_key {
            request_builder = request_builder.bearer_auth(api_key.expose_secret());
        }
        request_builder
    }

    pub async fn stream(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionStream, ChatCompletionError> {
        let client = self.clone();
        let stream = stream! {
            let mut attempt = 0;

            'retry: loop {
                let mut source = match EventSource::new(client.request(&request)) {
                    Ok(source) => source,
                    Err(error) => {
                        yield Err(ChatCompletionError::Stream(error.to_string()));
                        return;
                    }
                };
                source.set_retry_policy(Box::new(Never));

                let mut emitted_chunk = false;
                let mut saw_terminal = false;
                let mut retry_error = None;

                while let Some(event) = source.next().await {
                    match event {
                        Ok(Event::Open) => {}
                        Ok(Event::Message(message)) if message.data.trim() == "[DONE]" => {
                            saw_terminal = true;
                            break;
                        }
                        Ok(Event::Message(message)) => {
                            let Ok(chunk) = serde_json::from_str::<ChatCompletionChunk>(&message.data) else {
                                continue;
                            };
                            saw_terminal |= chunk.choices.iter().any(|choice| choice.finish_reason.is_some());
                            emitted_chunk = true;
                            yield Ok(chunk);
                        }
                        Err(reqwest_eventsource::Error::InvalidStatusCode(status, response)) => {
                            let body = match response.text().await {
                                Ok(body) => body,
                                Err(error) => {
                                    yield Err(ChatCompletionError::Network(error));
                                    return;
                                }
                            };
                            let error = classify_status(status.as_u16(), body);
                            if error.is_retryable() && !emitted_chunk && attempt < client.max_retries {
                                retry_error = Some(error);
                                break;
                            }
                            yield Err(error);
                            return;
                        }
                        Err(reqwest_eventsource::Error::Transport(error)) => {
                            yield Err(ChatCompletionError::Network(error));
                            return;
                        }
                        Err(reqwest_eventsource::Error::StreamEnded) => break,
                        Err(error) => {
                            yield Err(ChatCompletionError::Stream(error.to_string()));
                            return;
                        }
                    }
                }

                source.close();

                if retry_error.is_some() {
                    let multiplier = 2_u32.saturating_pow(attempt);
                    tokio::time::sleep(client.retry_delay.saturating_mul(multiplier)).await;
                    attempt += 1;
                    continue 'retry;
                }

                if !saw_terminal {
                    yield Err(ChatCompletionError::IncompleteStream);
                }
                return;
            }
        };

        Ok(Box::pin(stream))
    }
}

fn classify_status(status: u16, body: String) -> ChatCompletionError {
    match status {
        429 => ChatCompletionError::RateLimited { body },
        500 | 502 | 503 | 504 | 529 => ChatCompletionError::Overloaded { status, body },
        _ => ChatCompletionError::Api { status, body },
    }
}

impl ChatCompletionError {
    fn is_retryable(&self) -> bool {
        matches!(self, Self::RateLimited { .. } | Self::Overloaded { .. })
    }
}
