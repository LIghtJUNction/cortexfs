use std::{fmt, time::Duration};

use cortexfs_channels::{ChannelCodec, ChannelError, platform::telegram::TelegramCodec};
use serde_json::Value;

use super::bridge::{AgentChannelBridge, ChannelBridgeError};

mod api;

/// Foreground Telegram long-poll configuration.
pub struct TelegramConfig {
    token: String,
    api_base: String,
    poll_seconds: u64,
}

impl fmt::Debug for TelegramConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TelegramConfig")
            .field("token", &"[redacted]")
            .field("api_base", &self.api_base)
            .field("poll_seconds", &self.poll_seconds)
            .finish()
    }
}

impl TelegramConfig {
    pub fn new(
        token: impl Into<String>,
        api_base: impl Into<String>,
    ) -> Result<Self, TelegramError> {
        let token = token.into();
        let api_base = api_base.into();
        if token.is_empty() || api_base.is_empty() {
            return Err(TelegramError::Config(
                "token and API base are required".to_owned(),
            ));
        }
        Ok(Self {
            token,
            api_base,
            poll_seconds: 20,
        })
    }

    #[must_use]
    pub fn with_poll_seconds(mut self, seconds: u64) -> Self {
        self.poll_seconds = if seconds > 50 { 50 } else { seconds };
        self
    }
}

/// Errors returned by the built-in Telegram foreground adapter.
#[derive(Debug, thiserror::Error)]
pub enum TelegramError {
    #[error("invalid Telegram configuration: {0}")]
    Config(String),
    #[error("Telegram HTTP request failed")]
    Http(#[source] reqwest::Error),
    #[error("Telegram response JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Channel(#[from] ChannelError),
    #[error(transparent)]
    Bridge(#[from] ChannelBridgeError),
    #[error("Telegram API rejected request: {0}")]
    Api(String),
}

/// Runs one explicit foreground long-poll loop until the process is stopped.
pub fn run(config: &TelegramConfig, bridge: &AgentChannelBridge) -> Result<(), TelegramError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(config.poll_seconds.saturating_add(20)))
        .build()
        .map_err(TelegramError::Http)?;
    let codec = TelegramCodec;
    let mut offset = 0_i64;
    loop {
        let updates = api::get_updates(&client, config, offset)?;
        for update in updates {
            let payload = serde_json::to_string(&update)?;
            let update_id = update.get("update_id").and_then(Value::as_i64);
            let Some(inbound) = codec.decode(&payload)? else {
                if let Some(update_id) = update_id {
                    offset = offset.max(update_id.saturating_add(1));
                }
                continue;
            };
            let outbound = bridge.handle(inbound)?;
            api::send_message(&client, config, codec.encode(&outbound)?)?;
            if let Some(update_id) = update_id {
                offset = offset.max(update_id.saturating_add(1));
            }
        }
    }
}
