use std::{thread, time::Duration};

use cortexfs_channels::{ChannelCodec, platform::qq::QqCodec};
use reqwest::blocking::Client;
use serde_json::Value;
use tungstenite::Message;

use super::bridge::{AgentChannelBridge, ChannelBridgeError};

mod api;
mod config;
mod control;
mod gateway;

pub use config::{QqConfig, QqError};

/// Runs a reconnecting QQ Bot API gateway host.
pub fn run(config: &QqConfig, bridge: &AgentChannelBridge) -> Result<(), QqError> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(QqError::Http)?;
    let control = control::start(config, bridge, &client)?;
    loop {
        if let Err(_error) = gateway::run(&client, config, bridge, &control) {
            thread::sleep(config.reconnect_delay());
        }
    }
}

pub(super) fn handle_event(
    client: &Client,
    config: &QqConfig,
    bridge: &AgentChannelBridge,
    payload: &str,
) -> Result<(), QqError> {
    let root: Value = serde_json::from_str(payload)?;
    if root.get("t").and_then(Value::as_str).is_none() {
        return Ok(());
    }
    let Some(inbound) = QqCodec.decode(payload)? else {
        return Ok(());
    };
    if let Some(outbound) = ChannelBridgeError::consume_denied(bridge.handle(inbound))? {
        api::send(client, config, QqCodec.encode(&outbound)?)?;
    }
    Ok(())
}

pub(super) fn send(socket: &mut gateway::GatewaySocket, value: &Value) -> Result<(), QqError> {
    socket
        .send(Message::text(value.to_string()))
        .map_err(QqError::WebSocket)
}
