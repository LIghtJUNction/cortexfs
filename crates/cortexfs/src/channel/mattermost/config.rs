use std::{fmt, time::Duration};

/// Mattermost server and bot-token configuration.
pub struct MattermostConfig {
    pub(super) base_url: String,
    pub(super) token: String,
    pub(super) channels: Vec<String>,
    reconnect_seconds: u64,
}

impl MattermostConfig {
    pub fn new(
        base_url: impl Into<String>,
        token: impl Into<String>,
        channels: Vec<String>,
    ) -> Result<Self, MattermostError> {
        let config = Self {
            base_url: base_url.into().trim_end_matches('/').to_owned(),
            token: token.into(),
            channels,
            reconnect_seconds: 5,
        };
        if config.base_url.is_empty() || config.token.is_empty() {
            return Err(MattermostError::Config(
                "base URL and bot token are required".to_owned(),
            ));
        }
        url::Url::parse(&config.base_url)
            .map_err(|error| MattermostError::Config(error.to_string()))?;
        Ok(config)
    }

    #[must_use]
    pub fn with_reconnect_seconds(mut self, seconds: u64) -> Self {
        self.reconnect_seconds = seconds.clamp(1, 300);
        self
    }

    pub(super) fn reconnect_delay(&self) -> Duration {
        Duration::from_secs(self.reconnect_seconds)
    }

    pub(super) fn websocket_url(&self) -> String {
        let scheme = if self.base_url.starts_with("https://") {
            "wss://"
        } else {
            "ws://"
        };
        let host = self
            .base_url
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        format!("{scheme}{host}/api/v4/websocket")
    }

    pub(super) fn accepts(&self, channel: &str) -> bool {
        self.channels.is_empty() || self.channels.iter().any(|value| value == channel)
    }
}

impl fmt::Debug for MattermostConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MattermostConfig")
            .field("base_url", &self.base_url)
            .field("token", &"[redacted]")
            .field("channels", &self.channels)
            .field("reconnect_seconds", &self.reconnect_seconds)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MattermostError {
    #[error("invalid Mattermost configuration: {0}")]
    Config(String),
    #[error("Mattermost HTTP request failed")]
    Http(#[source] reqwest::Error),
    #[error("Mattermost JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Mattermost WebSocket failed: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error(transparent)]
    Channel(#[from] cortexfs_channels::ChannelError),
    #[error(transparent)]
    Bridge(#[from] super::super::bridge::ChannelBridgeError),
    #[error("Mattermost connection closed")]
    Closed,
    #[error("Mattermost protocol error: {0}")]
    Protocol(String),
}
