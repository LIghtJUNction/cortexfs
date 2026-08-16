use std::{io::ErrorKind, net::TcpStream, time::Duration};

use serde_json::json;
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

use super::{
    DiscordConfig, DiscordError,
    parse::{self, GatewayEvent},
};

pub(super) fn read_hello(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
) -> Result<Duration, DiscordError> {
    loop {
        match socket.read() {
            Ok(Message::Text(payload)) => {
                if let GatewayEvent::Hello(interval) = parse::parse(payload.as_str())? {
                    return Ok(interval);
                }
            }
            Ok(Message::Ping(payload)) => socket.send(Message::Pong(payload))?,
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(error) => return Err(DiscordError::WebSocket(error)),
        }
    }
}

pub(super) fn identify(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    config: &DiscordConfig,
) -> Result<(), DiscordError> {
    socket.send(Message::text(
        json!({
            "op": 2,
            "d": {
                "token": config.bot_token,
                "intents": config.intents,
                "properties": { "os": "linux", "browser": "cortexfs", "device": "cortexfs" }
            }
        })
        .to_string(),
    ))?;
    Ok(())
}

pub(super) fn heartbeat(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    sequence: Option<i64>,
) -> Result<(), DiscordError> {
    socket.send(Message::text(json!({ "op": 1, "d": sequence }).to_string()))?;
    Ok(())
}

pub(super) fn set_read_timeout(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    timeout: Duration,
) -> Result<(), std::io::Error> {
    match *socket.get_mut() {
        MaybeTlsStream::Plain(ref mut stream) => stream.set_read_timeout(Some(timeout)),
        MaybeTlsStream::Rustls(ref mut stream) => stream.sock.set_read_timeout(Some(timeout)),
        _ => Ok(()),
    }
}
