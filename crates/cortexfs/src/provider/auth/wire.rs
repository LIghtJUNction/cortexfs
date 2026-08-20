use serde::{Deserialize, Serialize};

/// Versioned local wire contract for one-shot authentication runners.
pub const AUTH_SOCKET_ABI: &str = "cortexfs.auth.socket/v1";

/// One local authentication request carried over a Unix socket pair.
#[derive(Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthWireRequest {
    ApiKey {
        request_id: String,
        provider: String,
        profile: String,
        key: String,
    },
    Device {
        request_id: String,
        provider: String,
        profile: String,
        base_url: String,
        methods: Vec<super::ProviderAuthConfig>,
        oauth: Box<crate::provider::oauth::OAuthProviderConfig>,
        timeout_secs: u64,
    },
    Browser {
        request_id: String,
        provider: String,
        profile: String,
        base_url: String,
        methods: Vec<super::ProviderAuthConfig>,
        oauth: Box<crate::provider::oauth::OAuthProviderConfig>,
        timeout_secs: u64,
    },
}

impl std::fmt::Debug for AuthWireRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::ApiKey {
                ref request_id,
                ref provider,
                ref profile,
                ..
            } => f
                .debug_struct("ApiKey")
                .field("request_id", request_id)
                .field("provider", provider)
                .field("profile", profile)
                .field("key", &"<redacted>")
                .finish(),
            Self::Device {
                ref request_id,
                ref provider,
                ref profile,
                ref base_url,
                timeout_secs,
                ..
            } => f
                .debug_struct("Device")
                .field("request_id", request_id)
                .field("provider", provider)
                .field("profile", profile)
                .field("base_url", base_url)
                .field("timeout_secs", &timeout_secs)
                .finish(),
            Self::Browser {
                ref request_id,
                ref provider,
                ref profile,
                ref base_url,
                timeout_secs,
                ..
            } => f
                .debug_struct("Browser")
                .field("request_id", request_id)
                .field("provider", provider)
                .field("profile", profile)
                .field("base_url", base_url)
                .field("timeout_secs", &timeout_secs)
                .finish(),
        }
    }
}

/// Sanitized progress or terminal result returned by an auth runner.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthWireResponse {
    Progress {
        request_id: String,
        state: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Result {
        request_id: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
    },
}

/// One JSONL frame at the authentication runner boundary.
#[derive(Debug, Deserialize, Serialize)]
pub struct AuthWireFrame<T> {
    pub abi: String,
    pub frame: T,
}

/// Stable frame errors that never include credential material.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AuthWireError {
    #[error("invalid authentication wire ABI")]
    Abi,
    #[error("invalid authentication wire frame")]
    Frame,
}

impl<T> AuthWireFrame<T> {
    #[must_use]
    pub fn new(frame: T) -> Self {
        Self {
            abi: AUTH_SOCKET_ABI.to_owned(),
            frame,
        }
    }
}

impl AuthWireFrame<AuthWireRequest> {
    pub fn decode(value: &str) -> Result<Self, AuthWireError> {
        let value = serde_json::from_str::<Self>(value).map_err(|_error| AuthWireError::Frame)?;
        (value.abi == AUTH_SOCKET_ABI)
            .then_some(value)
            .ok_or(AuthWireError::Abi)
    }
}

#[cfg(test)]
mod tests;
