use std::{fmt, time::Duration};

/// Matrix homeserver and access-token configuration.
pub struct MatrixConfig {
    pub(super) homeserver: String,
    pub(super) access_token: String,
    pub(super) rooms: Vec<String>,
    pub(super) sync_seconds: u64,
}

impl MatrixConfig {
    pub fn new(
        homeserver: impl Into<String>,
        access_token: impl Into<String>,
        rooms: Vec<String>,
    ) -> Result<Self, MatrixError> {
        let config = Self {
            homeserver: homeserver.into().trim_end_matches('/').to_owned(),
            access_token: access_token.into(),
            rooms,
            sync_seconds: 30,
        };
        if config.homeserver.is_empty() || config.access_token.is_empty() {
            return Err(MatrixError::Config(
                "homeserver and access token are required".to_owned(),
            ));
        }
        url::Url::parse(&config.homeserver)
            .map_err(|error| MatrixError::Config(error.to_string()))?;
        Ok(config)
    }

    #[must_use]
    pub fn with_sync_seconds(mut self, seconds: u64) -> Self {
        self.sync_seconds = seconds.clamp(1, 50);
        self
    }

    pub(super) fn timeout(&self) -> Duration {
        Duration::from_secs(self.sync_seconds)
    }
}

impl fmt::Debug for MatrixConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MatrixConfig")
            .field("homeserver", &self.homeserver)
            .field("access_token", &"[redacted]")
            .field("rooms", &self.rooms)
            .field("sync_seconds", &self.sync_seconds)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MatrixError {
    #[error("invalid Matrix configuration: {0}")]
    Config(String),
    #[error("Matrix HTTP request failed")]
    Http(#[source] reqwest::Error),
    #[error("Matrix JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Channel(#[from] cortexfs_channels::ChannelError),
    #[error(transparent)]
    Bridge(#[from] super::super::bridge::ChannelBridgeError),
    #[error("Matrix protocol error: {0}")]
    Protocol(String),
}
