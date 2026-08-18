use std::net::TcpStream;

use serde_json::json;
use tungstenite::{Message, WebSocket, stream::MaybeTlsStream};

use super::MattermostError;

pub(super) fn authenticate(
    socket: &mut WebSocket<MaybeTlsStream<TcpStream>>,
    token: &str,
) -> Result<(), MattermostError> {
    socket.send(Message::text(
        json!({
            "seq": 1,
            "action": "authentication_challenge",
            "data": {"token": token}
        })
        .to_string(),
    ))?;
    loop {
        let message = socket.read()?;
        let Message::Text(text) = message else {
            continue;
        };
        let value: serde_json::Value = serde_json::from_str(text.as_str())?;
        if value.get("status").and_then(serde_json::Value::as_str) == Some("OK") {
            return Ok(());
        }
        if value.get("error").is_some() {
            return Err(MattermostError::Protocol(
                "authentication challenge rejected".to_owned(),
            ));
        }
    }
}
