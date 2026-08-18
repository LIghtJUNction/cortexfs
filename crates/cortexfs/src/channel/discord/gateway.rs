use std::{
    io::ErrorKind,
    time::{Duration, Instant},
};

use cortexfs_channels::{ChannelCodec, platform::discord::DiscordCodec};
use reqwest::blocking::Client;
use tungstenite::{Message, connect, error::Error as WebSocketError};

use super::super::bridge::{AgentChannelBridge, ChannelProgressSink};
use super::transport;
use super::{
    DiscordConfig, DiscordError, api,
    parse::{self, GatewayEvent},
    progress,
};

const MAX_GATEWAY_MESSAGE_BYTES: usize = 256 * 1024;

pub(super) fn run(
    config: &DiscordConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
) -> Result<(), DiscordError> {
    let (mut socket, _) = connect(&config.gateway_url).map_err(DiscordError::WebSocket)?;
    socket.set_config(|value| {
        value.max_message_size = Some(MAX_GATEWAY_MESSAGE_BYTES);
        value.max_frame_size = Some(MAX_GATEWAY_MESSAGE_BYTES);
    });
    transport::set_read_timeout(&mut socket, Duration::from_secs(1))?;
    let interval = transport::read_hello(&mut socket)?;
    transport::identify(&mut socket, config)?;
    let mut sequence = None;
    let mut heartbeat_at = Instant::now() + interval;
    loop {
        if Instant::now() >= heartbeat_at {
            transport::heartbeat(&mut socket, sequence)?;
            heartbeat_at = Instant::now() + interval;
        }
        match socket.read() {
            Ok(Message::Text(payload)) => match parse::parse(payload.as_str())? {
                GatewayEvent::Dispatch {
                    name,
                    data,
                    sequence: next,
                } => {
                    sequence = next.or(sequence);
                    dispatch(config, bridge, client, &name, &data)?;
                }
                GatewayEvent::Heartbeat => transport::heartbeat(&mut socket, sequence)?,
                GatewayEvent::Reconnect | GatewayEvent::InvalidSession => return Ok(()),
                GatewayEvent::Hello(_) | GatewayEvent::Ignore => {}
            },
            Ok(Message::Ping(payload)) => socket.send(Message::Pong(payload))?,
            Ok(Message::Close(_)) => return Ok(()),
            Ok(_) => {}
            Err(WebSocketError::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(error) => return Err(DiscordError::WebSocket(error)),
        }
    }
}

fn dispatch(
    config: &DiscordConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
    name: &str,
    data: &serde_json::Value,
) -> Result<(), DiscordError> {
    let payload = serde_json::to_string(data)?;
    if name == "MESSAGE_CREATE" {
        let Some(inbound) = DiscordCodec.decode(&payload)? else {
            return Ok(());
        };
        let mut sink = progress::Progress::new(client, config, &inbound);
        return match bridge.handle_with_progress(inbound, &mut sink) {
            Ok(outbound) if !sink.completed() => api::send_reply(
                client,
                config,
                outbound.target.conversation.as_str(),
                &outbound.body.text,
            ),
            Ok(_) | Err(_) => Ok(()),
        };
    }
    if let Some(event) = DiscordCodec.decode_event(&payload)?
        && let Ok(outbound) = bridge.handle_event(&event)
    {
        api::send_reply(
            client,
            config,
            outbound.target.conversation.as_str(),
            &outbound.body.text,
        )?;
    }
    Ok(())
}
