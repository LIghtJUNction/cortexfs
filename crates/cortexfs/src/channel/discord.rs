use std::{path::PathBuf, time::Duration};

use cortexfs_channels::{ChannelError, ChannelId, ChannelProgressPolicy};

use super::bridge::{AgentChannelBridge, ChannelBridgeError};

#[derive(Clone)]
pub struct DiscordConfig {
    pub application_id: String,
    pub bot_token: String,
    pub agent_socket: PathBuf,
    pub agent: String,
    pub session_prefix: String,
    pub cwd: Option<String>,
    pub channel: Option<ChannelId>,
    pub api_base: String,
    pub gateway_url: String,
    pub intents: u64,
    pub progress: ChannelProgressPolicy,
}

impl std::fmt::Debug for DiscordConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscordConfig")
            .field("application_id", &self.application_id)
            .field("bot_token", &"[redacted]")
            .field("agent_socket", &self.agent_socket)
            .field("agent", &self.agent)
            .field("session_prefix", &self.session_prefix)
            .field("cwd", &self.cwd)
            .field("channel", &self.channel)
            .field("api_base", &self.api_base)
            .field("gateway_url", &self.gateway_url)
            .field("intents", &self.intents)
            .field("progress", &self.progress)
            .finish()
    }
}

mod api;
mod component;
mod control;
mod effect;
mod embed;
mod gateway;
mod invoke;
mod message;
mod parse;
mod progress;
mod request;
mod thread;
mod transport;
mod upload;

#[cfg(test)]
mod tests;

#[derive(Debug, thiserror::Error)]
pub enum DiscordError {
    #[error("Discord configuration failed: {0}")]
    Config(String),
    #[error("Discord HTTP request failed")]
    Http(#[source] reqwest::Error),
    #[error("Discord JSON failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Discord WebSocket failed: {0}")]
    WebSocket(#[from] tungstenite::Error),
    #[error("Discord I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("Discord authentication failed")]
    Authentication,
    #[error("Discord API rejected the request")]
    Api,
    #[error("Discord operation input is invalid: {0}")]
    Invalid(&'static str),
    #[error("Discord control driver failed: {0}")]
    Driver(#[from] cortexfs_channels::ChannelDriverError),
    #[error("Discord control runtime failed: {0}")]
    Runtime(#[source] super::driver::DriverError),
    #[error(transparent)]
    Channel(#[from] ChannelError),
    #[error(transparent)]
    Bridge(#[from] ChannelBridgeError),
    #[error("Discord gateway protocol error: {0}")]
    Protocol(String),
}

pub fn run(config: &DiscordConfig, bridge: &AgentChannelBridge) -> Result<(), DiscordError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(35))
        .user_agent(concat!("CortexFS/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(DiscordError::Http)?;
    api::verify_application(&client, config)?;
    let control = control::start(config, bridge, &client)?;
    loop {
        match gateway::run(config, bridge, &client, &control) {
            Ok(()) => reconnect_delay(),
            Err(error) => {
                reconnect_delay();
                return Err(error);
            }
        }
    }
}

pub(super) fn reconnect_delay() {
    std::thread::sleep(Duration::from_secs(5));
}
