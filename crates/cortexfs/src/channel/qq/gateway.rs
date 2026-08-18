use std::{io::ErrorKind, net::TcpStream, time::Duration};

use reqwest::blocking::Client;
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket, connect, stream::MaybeTlsStream};

use super::{QqConfig, QqError, api, send};
use crate::channel::bridge::AgentChannelBridge;

pub(super) type GatewaySocket = WebSocket<MaybeTlsStream<TcpStream>>;

pub(super) fn run(
    client: &Client,
    config: &QqConfig,
    bridge: &AgentChannelBridge,
) -> Result<(), QqError> {
    let url = api::gateway(client, config)?;
    let (mut socket, _) = connect(url).map_err(QqError::WebSocket)?;
    let interval = hello(&mut socket)?;
    set_read_timeout(&mut socket, interval)?;
    identify(&mut socket, config)?;
    let mut sequence = Value::Null;
    loop {
        match socket.read() {
            Ok(Message::Text(text)) => {
                let root: Value = serde_json::from_str(text.as_str())?;
                if let Some(value) = root.get("s") {
                    sequence = value.clone();
                }
                handle_opcode(
                    root.get("op").and_then(Value::as_u64),
                    &mut socket,
                    &sequence,
                )?;
                if root.get("op").and_then(Value::as_u64) == Some(0) {
                    super::handle_event(client, config, bridge, text.as_str())?;
                }
            }
            Ok(Message::Ping(payload)) => socket.send(Message::Pong(payload))?,
            Ok(Message::Close(_)) => return Err(QqError::Protocol("gateway closed".to_owned())),
            Ok(Message::Binary(_) | Message::Pong(_) | Message::Frame(_)) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::TimedOut | ErrorKind::WouldBlock) =>
            {
                send(&mut socket, &json!({"op":1,"d":sequence}))?;
            }
            Err(error) => return Err(QqError::WebSocket(error)),
        }
    }
}

fn hello(socket: &mut GatewaySocket) -> Result<Duration, QqError> {
    let Message::Text(text) = socket.read()? else {
        return Err(QqError::Protocol("gateway hello is missing".to_owned()));
    };
    let value: Value = serde_json::from_str(text.as_str())?;
    if value.get("op").and_then(Value::as_u64) != Some(10) {
        return Err(QqError::Protocol("unexpected gateway hello".to_owned()));
    }
    let millis = value
        .pointer("/d/heartbeat_interval")
        .and_then(Value::as_u64)
        .ok_or_else(|| QqError::Protocol("heartbeat interval is missing".to_owned()))?;
    Ok(Duration::from_millis(millis.clamp(1_000, 60_000)))
}

fn identify(socket: &mut GatewaySocket, config: &QqConfig) -> Result<(), QqError> {
    send(
        socket,
        &json!({
            "op": 2,
            "d": {
                "token": config.auth(),
                "intents": config.intents,
                "shard": [0, 1],
                "properties": {"$os":"linux","$browser":"cortexfs","$device":"cortexfs"}
            }
        }),
    )
}

fn handle_opcode(
    opcode: Option<u64>,
    socket: &mut GatewaySocket,
    sequence: &Value,
) -> Result<(), QqError> {
    match opcode {
        Some(1) => send(socket, &json!({"op":1,"d":sequence})),
        Some(7) => Err(QqError::Protocol("gateway requested reconnect".to_owned())),
        Some(9) => Err(QqError::Protocol("gateway invalid session".to_owned())),
        _ => Ok(()),
    }
}

fn set_read_timeout(socket: &mut GatewaySocket, timeout: Duration) -> Result<(), QqError> {
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "tungstenite exposes the stream as a mutable enum reference"
    )]
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => stream.set_read_timeout(Some(timeout)),
        MaybeTlsStream::Rustls(stream) => stream.sock.set_read_timeout(Some(timeout)),
        _ => return Err(QqError::Protocol("unsupported gateway stream".to_owned())),
    }
    .map_err(|error| QqError::Protocol(format!("set gateway timeout: {error}")))
}
