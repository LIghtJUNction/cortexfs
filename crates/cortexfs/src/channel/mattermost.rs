use std::{thread, time::Duration};

use reqwest::blocking::Client;
use tungstenite::{Message, connect};

use super::bridge::AgentChannelBridge;

mod api;
mod config;
mod control;
mod host;
mod transport;

pub use config::{MattermostConfig, MattermostError};

/// Runs a reconnecting Mattermost WebSocket and REST channel host.
pub fn run(config: &MattermostConfig, bridge: &AgentChannelBridge) -> Result<(), MattermostError> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(MattermostError::Http)?;
    let user_id = api::current_user(&client, config)?;
    let control = control::start(config, bridge, &client)?;
    loop {
        if let Err(_error) = run_connection(&client, config, bridge, &user_id, &control) {
            thread::sleep(config.reconnect_delay());
        }
    }
}

fn run_connection(
    client: &Client,
    config: &MattermostConfig,
    bridge: &AgentChannelBridge,
    user_id: &str,
    control: &crate::channel::control::ChannelControl,
) -> Result<(), MattermostError> {
    let (mut socket, _) = connect(config.websocket_url()).map_err(MattermostError::WebSocket)?;
    transport::authenticate(&mut socket, &config.token)?;
    loop {
        control
            .check()
            .map_err(|error| MattermostError::Protocol(error.to_string()))?;
        let message = socket.read().map_err(MattermostError::WebSocket)?;
        match message {
            Message::Text(text) => {
                host::handle_event(client, config, bridge, user_id, text.as_str())?;
            }
            Message::Ping(payload) => socket
                .send(Message::Pong(payload))
                .map_err(MattermostError::WebSocket)?,
            Message::Close(_) => return Err(MattermostError::Closed),
            Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }
}
