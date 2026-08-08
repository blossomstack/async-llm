//! OpenAI Responses API client and native protocol types.

pub mod chatgpt;
mod types;

use async_stream::stream;
use reqwest_eventsource::{retry::Never, Event, EventSource};
use secrecy::{ExposeSecret, SecretString};
use std::{pin::Pin, time::Duration};
use thiserror::Error;
use tokio_stream::{Stream, StreamExt as _};

pub use types::{
    CompletedResponse, FunctionTool, IncompleteDetails, IncompleteResponse, ReasoningControl,
    ResponseContentItem, ResponseOutputItem, ResponsesRequest, ResponsesStreamEvent,
    ResponsesUsage,
};

const DEFAULT_BASE_URL: &str = "https://api.openai.com";
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(120);
const DEFAULT_MAX_RETRIES: u32 = 6;
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(5);

pub type ResponsesStream =
    Pin<Box<dyn Stream<Item = Result<ResponsesStreamEvent, ResponsesError>> + Send>>;

#[derive(Debug, Error)]
pub enum ResponsesError {
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("rate limited: {body}")]
    RateLimited { body: String },

    #[error("provider overloaded (HTTP {status}): {body}")]
    Overloaded { status: u16, body: String },

    #[error("OpenAI Responses API returned HTTP {status}: {body}")]
    Api { status: u16, body: String },

    #[error("stream ended without a terminal frame")]
    IncompleteStream,

    #[error("ChatGPT credential error: {0}")]
    Authentication(String),

    #[error("stream error: {0}")]
    Stream(String),
}

impl ResponsesError {
    #[must_use]
    pub fn status(&self) -> Option<u16> {
        match self {
            Self::RateLimited { .. } => Some(429),
            Self::Overloaded { status, .. } | Self::Api { status, .. } => Some(*status),
            Self::Network(_)
            | Self::IncompleteStream
            | Self::Authentication(_)
            | Self::Stream(_) => None,
        }
    }

    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::RateLimited { .. } | Self::Overloaded { .. })
    }
}

#[derive(Clone, Debug)]
pub enum Credential {
    ApiKey(SecretString),
    ChatGpt(std::sync::Arc<chatgpt::ChatGptTokens>),
}

#[derive(Clone, Debug)]
pub struct Client {
    http_client: reqwest::Client,
    base_url: String,
    credential: Credential,
    max_retries: u32,
    retry_delay: Duration,
}

impl Client {
    #[must_use]
    pub fn with_api_key(key: impl Into<SecretString>) -> Self {
        Self {
            http_client: reqwest::Client::builder()
                .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
                .read_timeout(DEFAULT_READ_TIMEOUT)
                .build()
                .expect("default reqwest client configuration is valid"),
            base_url: DEFAULT_BASE_URL.into(),
            credential: Credential::ApiKey(key.into()),
            max_retries: DEFAULT_MAX_RETRIES,
            retry_delay: DEFAULT_RETRY_DELAY,
        }
    }

    #[must_use]
    pub fn with_chatgpt(tokens: std::sync::Arc<chatgpt::ChatGptTokens>) -> Self {
        Self {
            http_client: reqwest::Client::builder()
                .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
                .read_timeout(DEFAULT_READ_TIMEOUT)
                .build()
                .expect("default reqwest client configuration is valid"),
            base_url: "https://chatgpt.com".into(),
            credential: Credential::ChatGpt(tokens),
            max_retries: DEFAULT_MAX_RETRIES,
            retry_delay: DEFAULT_RETRY_DELAY,
        }
    }

    #[must_use]
    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into().trim_end_matches('/').into();
        self
    }

    #[must_use]
    pub fn max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    async fn request(
        &self,
        request: &ResponsesRequest,
    ) -> Result<reqwest::RequestBuilder, ResponsesError> {
        match &self.credential {
            Credential::ApiKey(key) => Ok(self
                .http_client
                .post(format!("{}/v1/responses", self.base_url))
                .bearer_auth(key.expose_secret())
                .json(request)),
            Credential::ChatGpt(tokens) => {
                let access_token = tokens.access_token().await?;
                let mut request_builder = self
                    .http_client
                    .post(format!("{}/backend-api/codex/responses", self.base_url))
                    .bearer_auth(access_token)
                    .json(request);
                if let Some(account_id) = tokens.account_id().await? {
                    request_builder = request_builder.header("chatgpt-account-id", account_id);
                }
                Ok(request_builder)
            }
        }
    }

    pub async fn stream(
        &self,
        request: ResponsesRequest,
    ) -> Result<ResponsesStream, ResponsesError> {
        let client = self.clone();
        let stream = stream! {
            let mut attempt = 0;

            'retry: loop {
                let request_builder = match client.request(&request).await {
                    Ok(request_builder) => request_builder,
                    Err(error) => {
                        yield Err(error);
                        return;
                    }
                };
                let mut source = match EventSource::new(request_builder) {
                    Ok(source) => source,
                    Err(error) => {
                        yield Err(ResponsesError::Stream(error.to_string()));
                        return;
                    }
                };
                source.set_retry_policy(Box::new(Never));

                let mut emitted_event = false;
                let mut saw_terminal = false;
                let mut retry_error = None;

                while let Some(event) = source.next().await {
                    match event {
                        Ok(Event::Open) => {}
                        Ok(Event::Message(message)) => {
                            let Ok(event) = serde_json::from_str::<ResponsesStreamEvent>(&message.data) else {
                                continue;
                            };
                            saw_terminal |= matches!(
                                event,
                                ResponsesStreamEvent::Completed { .. }
                                    | ResponsesStreamEvent::Incomplete { .. }
                                    | ResponsesStreamEvent::Failed { .. }
                            );
                            emitted_event = true;
                            yield Ok(event);
                            if saw_terminal {
                                break;
                            }
                        }
                        Err(reqwest_eventsource::Error::InvalidStatusCode(status, response)) => {
                            let body = match response.text().await {
                                Ok(body) => body,
                                Err(error) => {
                                    yield Err(ResponsesError::Network(error));
                                    return;
                                }
                            };
                            let error = classify_status(status.as_u16(), body);
                            if error.is_retryable() && !emitted_event && attempt < client.max_retries {
                                retry_error = Some(error);
                                break;
                            }
                            yield Err(error);
                            return;
                        }
                        Err(reqwest_eventsource::Error::Transport(error)) => {
                            yield Err(ResponsesError::Network(error));
                            return;
                        }
                        Err(reqwest_eventsource::Error::StreamEnded) => break,
                        Err(error) => {
                            yield Err(ResponsesError::Stream(error.to_string()));
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
                    yield Err(ResponsesError::IncompleteStream);
                }
                return;
            }
        };

        Ok(Box::pin(stream))
    }
}

fn classify_status(status: u16, body: String) -> ResponsesError {
    match status {
        429 => ResponsesError::RateLimited { body },
        500 | 502 | 503 | 504 | 529 => ResponsesError::Overloaded { status, body },
        _ => ResponsesError::Api { status, body },
    }
}
