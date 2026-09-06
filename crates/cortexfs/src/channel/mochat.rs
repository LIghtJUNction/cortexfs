use std::{thread, time::Duration};

use cortexfs_channels::{ChannelCodec, platform::mochat::MochatCodec};
use reqwest::blocking::Client;
use serde_json::Value;

use super::bridge::{AgentChannelBridge, ChannelBridgeError};

mod api;
mod config;
mod control;

#[cfg(test)]
mod tests;

pub use config::MochatConfig;

/// Runs a reconnecting Mochat HTTP message poller.
pub fn run(config: &MochatConfig, bridge: &AgentChannelBridge) -> Result<(), MochatError> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(MochatError::Http)?;
    let control = control::start(config, bridge, &client)?;
    let mut since_id = None;
    loop {
        control
            .check()
            .map_err(|error| MochatError::Protocol(error.to_string()))?;
        match poll(&client, config, bridge, &mut since_id) {
            Ok(()) => thread::sleep(config.poll_delay()),
            Err(_error) => thread::sleep(Duration::from_secs(5)),
        }
    }
}

fn poll(
    client: &Client,
    config: &MochatConfig,
    bridge: &AgentChannelBridge,
    since_id: &mut Option<String>,
) -> Result<(), MochatError> {
    let codec = MochatCodec;
    let payload = api::receive(client, config, since_id.as_deref())?;
    let root: Value =
        serde_json::from_str(&payload).map_err(|error| MochatError::Protocol(error.to_string()))?;
    for inbound in codec.decode_many(&payload)?.into_iter().rev() {
        advance(
            since_id,
            inbound
                .metadata
                .get("mochat.message_id")
                .map(String::as_str),
        );
        if !config.accepts(&inbound.sender.id) {
            continue;
        }
        if let Some(outbound) = ChannelBridgeError::consume_denied(bridge.handle(inbound))? {
            api::send(client, config, codec.encode(&outbound)?)?;
        }
    }
    advance(
        since_id,
        root.get("meta")
            .and_then(|meta| meta.get("next_id"))
            .and_then(Value::as_str),
    );
    Ok(())
}

fn advance(cursor: &mut Option<String>, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty())
        && cursor.as_deref().is_none_or(|current| value > current)
    {
        *cursor = Some(value.to_owned());
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MochatError {
    #[error("Mochat configuration failed: {0}")]
    Config(String),
    #[error("Mochat HTTP request failed")]
    Http(#[source] reqwest::Error),
    #[error(transparent)]
    Channel(#[from] cortexfs_channels::ChannelError),
    #[error(transparent)]
    Bridge(#[from] ChannelBridgeError),
    #[error("Mochat response is invalid: {0}")]
    Protocol(String),
}
