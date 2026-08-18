use std::{fmt, time::Duration};

use cortexfs_channels::ChannelError;

use super::super::bridge::ChannelBridgeError;

/// Bluesky handle, app-password, and polling configuration.
pub struct BlueskyConfig {
    pub(super) handle: String,
    pub(super) app_password: String,
    pub(super) api_base: String,
    poll_seconds: u64,
}

impl BlueskyConfig {
    pub fn new(
        handle: impl Into<String>,
        app_password: impl Into<String>,
        api_base: impl Into<String>,
    ) -> Result<Self, BlueskyError> {
        let config = Self {
            handle: handle.into(),
            app_password: app_password.into(),
            api_base: api_base.into().trim_end_matches('/').to_owned(),
            poll_seconds: 5,
        };
        if [
            config.handle.as_str(),
            config.app_password.as_str(),
            config.api_base.as_str(),
        ]
        .iter()
        .any(|value| value.is_empty() || value.contains('\0'))
        {
            return Err(BlueskyError::Config(
                "handle, app password, and API base are required".to_owned(),
            ));
        }
        url::Url::parse(&config.api_base)
            .map_err(|error| BlueskyError::Config(error.to_string()))?;
        Ok(config)
    }

    #[must_use]
    pub fn with_poll_seconds(mut self, seconds: u64) -> Self {
        self.poll_seconds = seconds.clamp(1, 300);
        self
    }

    pub(super) fn poll_delay(&self) -> Duration {
        Duration::from_secs(self.poll_seconds)
    }
}

impl fmt::Debug for BlueskyConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BlueskyConfig")
            .field("handle", &self.handle)
            .field("app_password", &"[redacted]")
            .field("api_base", &self.api_base)
            .field("poll_seconds", &self.poll_seconds)
            .finish()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BlueskyError {
    #[error("invalid Bluesky configuration: {0}")]
    Config(String),
    #[error("Bluesky HTTP request failed")]
    Http(#[source] reqwest::Error),
    #[error(transparent)]
    Channel(#[from] ChannelError),
    #[error(transparent)]
    Bridge(#[from] ChannelBridgeError),
    #[error("Bluesky authentication failed")]
    Unauthorized,
    #[error("Bluesky protocol error: {0}")]
    Protocol(String),
}
