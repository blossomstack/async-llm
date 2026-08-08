//! OpenAI Responses API client and native protocol types.

pub mod chatgpt;
mod types;

use async_stream::stream;
use reqwest::StatusCode;
use secrecy::{ExposeSecret, SecretString};
use std::{pin::Pin, time::Duration};
use thiserror::Error;
use tokio_stream::{Stream, StreamExt as _};

pub use types::{
    CompletedResponse, FunctionTool, IncompleteDetails, IncompleteResponse, ReasoningControl,
    ResponseContentItem, ResponseErrorDetails, ResponseOutputItem, ResponsesRequest,
    ResponsesStreamEvent, ResponsesUsage,
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
            http_client: default_http_client(),
            base_url: DEFAULT_BASE_URL.into(),
            credential: Credential::ApiKey(key.into()),
            max_retries: DEFAULT_MAX_RETRIES,
            retry_delay: DEFAULT_RETRY_DELAY,
        }
    }

    #[must_use]
    pub fn with_chatgpt(tokens: std::sync::Arc<chatgpt::ChatGptTokens>) -> Self {
        Self {
            http_client: default_http_client(),
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

    async fn refresh_chatgpt_tokens(&self) -> Result<(), ResponsesError> {
        match &self.credential {
            Credential::ChatGpt(tokens) => tokens.refresh().await,
            Credential::ApiKey(_) => Ok(()),
        }
    }

    pub async fn stream(
        &self,
        request: ResponsesRequest,
    ) -> Result<ResponsesStream, ResponsesError> {
        let client = self.clone();
        let stream = stream! {
            let mut attempt = 0;
            let mut refreshed_after_unauthorized = false;

            'retry: loop {
                let response = match client.request(&request).await {
                    Ok(request_builder) => match request_builder.send().await {
                        Ok(response) => response,
                        Err(error) => {
                            yield Err(ResponsesError::Network(error));
                            return;
                        }
                    },
                    Err(error) => {
                        yield Err(error);
                        return;
                    }
                };

                if !response.status().is_success() {
                    let status = response.status();
                    let body = match response.text().await {
                        Ok(body) => body,
                        Err(error) => {
                            yield Err(ResponsesError::Network(error));
                            return;
                        }
                    };
                    if status == StatusCode::UNAUTHORIZED
                        && matches!(client.credential, Credential::ChatGpt(_))
                        && !refreshed_after_unauthorized
                    {
                        if let Err(error) = client.refresh_chatgpt_tokens().await {
                            yield Err(error);
                            return;
                        }
                        refreshed_after_unauthorized = true;
                        continue 'retry;
                    }
                    let error = classify_status(status.as_u16(), body);
                    if error.is_retryable() && attempt < client.max_retries {
                        sleep_for_retry(&client, attempt).await;
                        attempt += 1;
                        continue 'retry;
                    }
                    yield Err(error);
                    return;
                }

                let mut body = response.bytes_stream();
                let mut buffer = Vec::new();
                let mut saw_terminal = false;
                while let Some(chunk) = body.next().await {
                    let chunk = match chunk {
                        Ok(chunk) => chunk,
                        Err(error) => {
                            yield Err(ResponsesError::Network(error));
                            return;
                        }
                    };
                    buffer.extend_from_slice(&chunk);
                    let frames = match take_sse_frames(&mut buffer) {
                        Ok(frames) => frames,
                        Err(error) => {
                            yield Err(error);
                            return;
                        }
                    };
                    for frame in frames {
                        let Some(data) = sse_data(&frame) else {
                            continue;
                        };
                        let Ok(event) = serde_json::from_str::<ResponsesStreamEvent>(&data) else {
                            continue;
                        };
                        saw_terminal |= matches!(
                            event,
                            ResponsesStreamEvent::Completed { .. }
                                | ResponsesStreamEvent::Incomplete { .. }
                                | ResponsesStreamEvent::Failed { .. }
                                | ResponsesStreamEvent::Error { .. }
                        );
                        yield Ok(event);
                        if saw_terminal {
                            break;
                        }
                    }
                    if saw_terminal {
                        break;
                    }
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

fn default_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
        .read_timeout(DEFAULT_READ_TIMEOUT)
        .build()
        .expect("default reqwest client configuration is valid")
}

async fn sleep_for_retry(client: &Client, attempt: u32) {
    let multiplier = 2_u32.saturating_pow(attempt);
    tokio::time::sleep(client.retry_delay.saturating_mul(multiplier)).await;
}

fn take_sse_frames(buffer: &mut Vec<u8>) -> Result<Vec<String>, ResponsesError> {
    let mut frames = Vec::new();
    loop {
        let Some((index, separator_len)) = find_sse_separator(buffer) else {
            return Ok(frames);
        };
        let frame: Vec<u8> = buffer.drain(..index).collect();
        buffer.drain(..separator_len);
        frames.push(
            String::from_utf8(frame).map_err(|error| ResponsesError::Stream(error.to_string()))?,
        );
    }
}

fn find_sse_separator(buffer: &[u8]) -> Option<(usize, usize)> {
    buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| (index, 4))
        .or_else(|| {
            buffer
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| (index, 2))
        })
}

fn sse_data(frame: &str) -> Option<String> {
    let data: Vec<_> = frame
        .lines()
        .filter_map(|line| line.strip_prefix("data:"))
        .map(|line| line.strip_prefix(' ').unwrap_or(line))
        .collect();
    (!data.is_empty()).then(|| data.join("\n"))
}

fn classify_status(status: u16, body: String) -> ResponsesError {
    match status {
        429 => ResponsesError::RateLimited { body },
        500 | 502 | 503 | 504 | 529 => ResponsesError::Overloaded { status, body },
        _ => ResponsesError::Api { status, body },
    }
}
