use std::{fmt, time::Duration};

/// QQ Bot application and gateway configuration.
#[derive(Clone)]
pub struct QqConfig {
    pub(super) app_id: String,
    pub(super) token: String,
    pub(super) api_base: String,
    pub(super) gateway_url: String,
    pub(super) intents: u64,
    reconnect_seconds: u64,
}

impl QqConfig {
    pub fn new(
        app_id: impl Into<String>,
        token: impl Into<String>,
        api_base: impl Into<String>,
        gateway_url: impl Into<String>,
    ) -> Result<Self, QqError> {
        let config = Self {
            app_id: app_id.into(),
            token: token.into(),
            api_base: api_base.into().trim_end_matches('/').to_owned(),
            gateway_url: gateway_url.into(),
            intents: (1 << 25) | (1 << 30),
            reconnect_seconds: 5,
        };
        if [
            config.app_id.as_str(),
            config.token.as_str(),
            config.api_base.as_str(),
            config.gateway_url.as_str(),
        ]
        .iter()
        .any(|value| value.is_empty() || value.contains('\0'))
        {
            return Err(QqError::Config(
                "QQ credentials and endpoints are required".to_owned(),
            ));
        }
        url::Url::parse(&config.api_base)
            .and_then(|_| url::Url::parse(&config.gateway_url))
            .map_err(|error| QqError::Config(error.to_string()))?;
        Ok(config)
    }

    #[must_use]
    pub fn with_intents(mut self, intents: u64) -> Self {
        self.intents = intents;
        self
    }

    #[must_use]
    pub fn with_reconnect_seconds(mut self, seconds: u64) -> Self {
        self.reconnect_seconds = seconds.clamp(1, 300);
        self
    }

    pub(super) fn auth(&self) -> String {
        format!("Bot {}.{}", self.app_id, self.token)
    }

    pub(super) fn reconnect_delay(&self) -> Duration {
        Duration::from_secs(self.reconnect_seconds)
    }
}

impl fmt::Debug for QqConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QqConfig")
            .field("app_id", &self.app_id)
            .field("token", &"[redacted]")
            .field("api_base", &self.api_base)
            .field("gateway_url", &self.gateway_url)
            .field("intents", &self.intents)
            .field("reconnect_seconds", &self.reconnect_seconds)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum QqError {
    #[error("invalid QQ configuration: {0}")]
    Config(String),
    #[error("QQ HTTP request failed")]
    Http(#[source] reqwest::Error),
    #[error("QQ JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("QQ WebSocket failed: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error(transparent)]
    Channel(#[from] cortexfs_channels::ChannelError),
    #[error(transparent)]
    Bridge(#[from] super::super::bridge::ChannelBridgeError),
    #[error("QQ gateway protocol error: {0}")]
    Protocol(String),
}
