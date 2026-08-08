//! ChatGPT device-login and refreshable credential support.

use crate::responses::ResponsesError;
use async_trait::async_trait;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

pub const DEFAULT_ISSUER: &str = "https://auth.openai.com";

/// Persisted ChatGPT OAuth tokens and their selected account identifier.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct StoredTokens {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
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
        })
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
    issuer: String,
    client_id: String,
}

impl std::fmt::Debug for ChatGptTokens {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ChatGptTokens")
            .field("issuer", &self.issuer)
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

impl ChatGptTokens {
    #[must_use]
    pub fn with_store_and_issuer(
        tokens: StoredTokens,
        store: Arc<dyn TokenStore>,
        issuer: impl Into<String>,
        client_id: impl Into<String>,
    ) -> Self {
        Self {
            tokens: RwLock::new(tokens),
            store,
            issuer: issuer.into().trim_end_matches('/').into(),
            client_id: client_id.into(),
        }
    }

    pub async fn access_token(&self) -> Result<String, ResponsesError> {
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
        let refresh_token = self
            .tokens
            .read()
            .map_err(|_| ResponsesError::Authentication("token lock poisoned".into()))
            .map(|tokens| tokens.refresh_token.clone())?;
        let response = reqwest::Client::new()
            .post(format!("{}/oauth/token", self.issuer))
            .json(&serde_json::json!({
                "grant_type": "refresh_token",
                "refresh_token": refresh_token,
                "client_id": self.client_id,
            }))
            .send()
            .await?;
        let tokens = parse_token_response(response).await?;
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
    #[serde(default)]
    pub verification_uri: String,
    #[serde(default)]
    pub interval: u64,
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

/// Starts the ChatGPT device authorization flow against an issuer.
pub async fn start_device_login(
    http_client: &reqwest::Client,
    issuer: impl AsRef<str>,
    client_id: impl Into<String>,
) -> Result<DeviceLogin, ResponsesError> {
    let issuer = issuer.as_ref().trim_end_matches('/');
    let client_id = client_id.into();
    let response = http_client
        .post(format!("{issuer}/api/accounts/deviceauth/usercode"))
        .json(&serde_json::json!({"client_id": client_id}))
        .send()
        .await?;
    parse_json_response(response).await
}

/// Polls a device authorization flow and returns refreshable tokens.
pub async fn poll_device_login(
    http_client: &reqwest::Client,
    issuer: impl AsRef<str>,
    client_id: impl Into<String>,
    login: &DeviceLogin,
    store: Arc<dyn TokenStore>,
) -> Result<Arc<ChatGptTokens>, ResponsesError> {
    let issuer = issuer.as_ref().trim_end_matches('/').to_string();
    let client_id = client_id.into();
    let response = http_client
        .post(format!("{issuer}/api/accounts/deviceauth/token"))
        .json(&login.poll_request())
        .send()
        .await?;
    let tokens = parse_token_response(response).await?;
    store.save(tokens.clone()).await?;
    Ok(Arc::new(ChatGptTokens::with_store_and_issuer(
        tokens, store, issuer, client_id,
    )))
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    #[serde(default)]
    id_token: Option<String>,
}

async fn parse_token_response(response: reqwest::Response) -> Result<StoredTokens, ResponsesError> {
    let response = parse_json_response::<TokenResponse>(response).await?;
    let account_id = response
        .id_token
        .as_deref()
        .map(account_id_from_id_token)
        .transpose()?
        .flatten();
    Ok(StoredTokens {
        access_token: response.access_token,
        refresh_token: response.refresh_token,
        id_token: response.id_token,
        account_id,
    })
}

async fn parse_json_response<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, ResponsesError> {
    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        return Err(ResponsesError::Api {
            status: status.as_u16(),
            body,
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
        .get("https://api.openai.com/auth")
        .and_then(|auth| auth.get("chatgpt_account_id"))
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            claims
                .get("chatgpt_account_id")
                .and_then(serde_json::Value::as_str)
        })
        .or_else(|| claims.get("account_id").and_then(serde_json::Value::as_str))
        .map(ToOwned::to_owned))
}
