//! ChatGPT device-login and refreshable credential support.

use crate::responses::ResponsesError;
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use std::{
    sync::{Arc, RwLock},
    time::{SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_ISSUER: &str = "https://auth.openai.com";
const EXPIRY_SKEW_SECONDS: u64 = 60;
/// OpenAI's documented default when a token response omits `expires_in`.
/// Treating an absent value as "never expires" would strand the credential:
/// nothing would ever refresh it, and every call after the hour would 401.
const DEFAULT_EXPIRES_IN_SECONDS: u64 = 3600;
const DEFAULT_POLL_INTERVAL_SECONDS: u64 = 5;

/// Identifies the calling client to OpenAI's ChatGPT auth and Codex endpoints.
///
/// The device-auth endpoints are not the public OAuth ones: they key their
/// behaviour off `client_id` and off an `originator` header naming the tool
/// making the request.
#[derive(Clone, Debug)]
pub struct ChatGptAuth {
    issuer: String,
    client_id: String,
    originator: Option<String>,
}

impl ChatGptAuth {
    #[must_use]
    pub fn new(client_id: impl Into<String>) -> Self {
        Self {
            issuer: DEFAULT_ISSUER.into(),
            client_id: client_id.into(),
            originator: None,
        }
    }

    /// Point the flow at a different issuer. Tests only — there is one OpenAI.
    #[must_use]
    pub fn with_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = issuer.into().trim_end_matches('/').into();
        self
    }

    #[must_use]
    pub fn with_originator(mut self, originator: impl Into<String>) -> Self {
        self.originator = Some(originator.into());
        self
    }

    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    #[must_use]
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    #[must_use]
    pub fn originator(&self) -> Option<&str> {
        self.originator.as_deref()
    }

    /// Where the person approves a device login. OpenAI's user-code response
    /// does not carry it, so it is derived rather than read.
    #[must_use]
    pub fn verification_url(&self) -> String {
        format!("{}/codex/device", self.issuer)
    }

    fn identify(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.originator {
            Some(originator) => builder.header("originator", originator),
            None => builder,
        }
    }
}

/// Persisted ChatGPT OAuth tokens and their selected account identifier.
#[derive(Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct StoredTokens {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<u64>,
}

impl std::fmt::Debug for StoredTokens {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredTokens")
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("id_token", &self.id_token.as_ref().map(|_| "<redacted>"))
            .field("account_id", &self.account_id)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl StoredTokens {
    pub fn new(
        access_token: impl Into<String>,
        refresh_token: impl Into<String>,
        id_token: impl Into<String>,
    ) -> Result<Self, ResponsesError> {
        let id_token = id_token.into();
        let account_id = account_id_from_id_token(&id_token)?;
        Ok(Self {
            access_token: access_token.into(),
            refresh_token: refresh_token.into(),
            id_token: Some(id_token),
            account_id,
            expires_at: None,
        })
    }

    fn expires_soon(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| expires_at <= unix_time() + EXPIRY_SKEW_SECONDS)
    }
}

/// Storage used to load and persist refreshable ChatGPT credentials.
#[async_trait]
pub trait TokenStore: Send + Sync {
    async fn load(&self) -> Result<Option<StoredTokens>, ResponsesError>;
    async fn save(&self, tokens: StoredTokens) -> Result<(), ResponsesError>;
}

/// Refreshable ChatGPT OAuth tokens.
pub struct ChatGptTokens {
    tokens: RwLock<StoredTokens>,
    store: Arc<dyn TokenStore>,
    auth: ChatGptAuth,
}

impl std::fmt::Debug for ChatGptTokens {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChatGptTokens")
            .field("auth", &self.auth)
            .finish_non_exhaustive()
    }
}

impl ChatGptTokens {
    #[must_use]
    pub fn new(tokens: StoredTokens, store: Arc<dyn TokenStore>, auth: ChatGptAuth) -> Self {
        Self {
            tokens: RwLock::new(tokens),
            store,
            auth,
        }
    }

    #[must_use]
    pub fn auth(&self) -> &ChatGptAuth {
        &self.auth
    }

    pub async fn access_token(&self) -> Result<String, ResponsesError> {
        let expires_soon = self
            .tokens
            .read()
            .map_err(|_| ResponsesError::Authentication("token lock poisoned".into()))?
            .expires_soon();
        if expires_soon {
            self.refresh().await?;
        }
        self.tokens
            .read()
            .map_err(|_| ResponsesError::Authentication("token lock poisoned".into()))
            .map(|tokens| tokens.access_token.clone())
    }

    pub async fn account_id(&self) -> Result<Option<String>, ResponsesError> {
        self.tokens
            .read()
            .map_err(|_| ResponsesError::Authentication("token lock poisoned".into()))
            .map(|tokens| tokens.account_id.clone())
    }

    /// Exchanges the stored refresh token and persists the replacement tokens.
    pub async fn refresh(&self) -> Result<(), ResponsesError> {
        let existing = self
            .tokens
            .read()
            .map_err(|_| ResponsesError::Authentication("token lock poisoned".into()))?
            .clone();
        let response = self
            .auth
            .identify(
                reqwest::Client::new()
                    .post(format!("{}/oauth/token", self.auth.issuer))
                    .form(&[
                        ("grant_type", "refresh_token"),
                        ("refresh_token", existing.refresh_token.as_str()),
                        ("client_id", self.auth.client_id.as_str()),
                    ]),
            )
            .send()
            .await?;
        let tokens = parse_token_response(response, Some(&existing)).await?;
        self.store.save(tokens.clone()).await?;
        *self
            .tokens
            .write()
            .map_err(|_| ResponsesError::Authentication("token lock poisoned".into()))? = tokens;
        Ok(())
    }
}

/// The device code displayed to a user during ChatGPT login.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct DeviceLogin {
    pub device_auth_id: String,
    pub user_code: String,
    /// Where the person approves this login. OpenAI's user-code response omits
    /// it, so `start_device_login` derives it from the issuer.
    #[serde(default)]
    pub verification_uri: String,
    /// Seconds to wait between polls. OpenAI sends this as a JSON *string*, so
    /// it is read leniently; under a second would rate-limit the very login it
    /// is trying to complete.
    #[serde(
        default = "default_poll_interval",
        deserialize_with = "deserialize_poll_interval"
    )]
    pub interval: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code_verifier: Option<String>,
}

fn default_poll_interval() -> u64 {
    DEFAULT_POLL_INTERVAL_SECONDS
}

fn deserialize_poll_interval<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let seconds = match &value {
        serde_json::Value::String(text) => text.parse::<u64>().ok(),
        other => other.as_u64(),
    };
    Ok(seconds.unwrap_or(DEFAULT_POLL_INTERVAL_SECONDS).max(1))
}

impl DeviceLogin {
    #[must_use]
    pub fn new(
        device_auth_id: impl Into<String>,
        user_code: impl Into<String>,
        verification_uri: impl Into<String>,
        interval: u64,
    ) -> Self {
        Self {
            device_auth_id: device_auth_id.into(),
            user_code: user_code.into(),
            verification_uri: verification_uri.into(),
            interval,
            code_verifier: None,
        }
    }

    #[must_use]
    pub fn authorization_request(&self, client_id: impl Into<String>) -> serde_json::Value {
        serde_json::json!({"client_id": client_id.into()})
    }

    #[must_use]
    pub fn poll_request(&self) -> serde_json::Value {
        serde_json::json!({
            "device_auth_id": self.device_auth_id,
            "user_code": self.user_code,
        })
    }
}

/// The current state of a device login poll.
#[derive(Debug)]
pub enum DeviceLoginPoll {
    Pending,
    Approved(Arc<ChatGptTokens>),
}

/// Starts the ChatGPT device authorization flow.
pub async fn start_device_login(
    http_client: &reqwest::Client,
    auth: &ChatGptAuth,
) -> Result<DeviceLogin, ResponsesError> {
    let response = auth
        .identify(
            http_client
                .post(format!(
                    "{}/api/accounts/deviceauth/usercode",
                    auth.issuer()
                ))
                .json(&serde_json::json!({"client_id": auth.client_id()})),
        )
        .send()
        .await?;
    let mut login: DeviceLogin = parse_json_response(response).await?;
    if login.verification_uri.is_empty() {
        login.verification_uri = auth.verification_url();
    }
    Ok(login)
}

/// Polls a device authorization flow. Pending authorization is not an error.
///
/// The device-auth endpoint answers 4xx for the whole window in which the
/// person has not yet approved the code, so an unsuccessful poll is reported as
/// [`DeviceLoginPoll::Pending`] rather than as a failure — a login that is
/// merely unfinished must not look like a broken one.
pub async fn poll_device_login(
    http_client: &reqwest::Client,
    auth: &ChatGptAuth,
    login: &DeviceLogin,
    store: Arc<dyn TokenStore>,
) -> Result<DeviceLoginPoll, ResponsesError> {
    let response = auth
        .identify(
            http_client
                .post(format!("{}/api/accounts/deviceauth/token", auth.issuer()))
                .json(&login.poll_request()),
        )
        .send()
        .await?;
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Ok(DeviceLoginPoll::Pending);
    }
    // An approval that carries neither code nor verifier is the same
    // "not finished yet" state expressed as a 200.
    let Ok(approval) = serde_json::from_str::<DeviceApproval>(&body) else {
        return Ok(DeviceLoginPoll::Pending);
    };
    let Some(code_verifier) = approval
        .code_verifier
        .or_else(|| login.code_verifier.clone())
    else {
        return Ok(DeviceLoginPoll::Pending);
    };
    // OpenAI's own device callback. Nothing ever navigates to it — declaring it
    // is the protocol formality that lets a server with no reachable redirect
    // complete the exchange at all.
    let redirect_uri = format!("{}/deviceauth/callback", auth.issuer());
    let response = auth
        .identify(
            http_client
                .post(format!("{}/oauth/token", auth.issuer()))
                .form(&[
                    ("grant_type", "authorization_code"),
                    ("client_id", auth.client_id()),
                    ("code", approval.authorization_code.as_str()),
                    ("redirect_uri", redirect_uri.as_str()),
                    ("code_verifier", code_verifier.as_str()),
                ]),
        )
        .send()
        .await?;
    let tokens = parse_token_response(response, None).await?;
    store.save(tokens.clone()).await?;
    Ok(DeviceLoginPoll::Approved(Arc::new(ChatGptTokens::new(
        tokens,
        store,
        auth.clone(),
    ))))
}

#[derive(Deserialize)]
struct DeviceApproval {
    authorization_code: String,
    #[serde(default)]
    code_verifier: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    #[serde(default)]
    expires_in: Option<u64>,
}

async fn parse_token_response(
    response: reqwest::Response,
    existing: Option<&StoredTokens>,
) -> Result<StoredTokens, ResponsesError> {
    let response = parse_json_response::<TokenResponse>(response).await?;
    let has_id_token = response.id_token.is_some();
    let id_token = response
        .id_token
        .or_else(|| existing.and_then(|tokens| tokens.id_token.clone()));
    let account_id = if has_id_token {
        id_token
            .as_deref()
            .map(account_id_from_id_token)
            .transpose()?
            .flatten()
    } else {
        existing.and_then(|tokens| tokens.account_id.clone())
    };
    Ok(StoredTokens {
        access_token: response.access_token,
        refresh_token: response
            .refresh_token
            .or_else(|| existing.map(|tokens| tokens.refresh_token.clone()))
            .ok_or_else(|| {
                ResponsesError::Authentication("token response omitted refresh_token".into())
            })?,
        id_token,
        account_id,
        expires_at: Some(
            unix_time().saturating_add(response.expires_in.unwrap_or(DEFAULT_EXPIRES_IN_SECONDS)),
        ),
    })
}

async fn parse_json_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, ResponsesError> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        // A 5xx from the auth endpoint is the endpoint being unwell, not the
        // credential being rejected. Keeping the two apart is what lets a
        // caller retry instead of discarding a refresh token that still works.
        return Err(if status.as_u16() >= 500 {
            ResponsesError::Overloaded {
                status: status.as_u16(),
                body,
            }
        } else {
            ResponsesError::Api {
                status: status.as_u16(),
                body,
            }
        });
    }
    serde_json::from_str(&body)
        .map_err(|error| ResponsesError::Authentication(format!("invalid auth response: {error}")))
}

fn account_id_from_id_token(id_token: &str) -> Result<Option<String>, ResponsesError> {
    let payload = id_token
        .split('.')
        .nth(1)
        .ok_or_else(|| ResponsesError::Authentication("invalid ID token".into()))?;
    let bytes = URL_SAFE_NO_PAD.decode(payload).map_err(|error| {
        ResponsesError::Authentication(format!("invalid ID token payload: {error}"))
    })?;
    let claims: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
        ResponsesError::Authentication(format!("invalid ID token claims: {error}"))
    })?;

    Ok(claims
        .get("chatgpt_account_id")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            claims
                .get("https://api.openai.com/auth")
                .and_then(|auth| auth.get("chatgpt_account_id"))
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| {
            claims
                .get("organizations")
                .and_then(serde_json::Value::as_array)
                .and_then(|organizations| organizations.first())
                .and_then(|organization| organization.get("id"))
                .and_then(serde_json::Value::as_str)
        })
        .map(ToOwned::to_owned))
}

fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
