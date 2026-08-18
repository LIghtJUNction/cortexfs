#![expect(
    clippy::pattern_type_mismatch,
    reason = "matching borrowed Slack payloads avoids cloning event bodies"
)]

use cortexfs_channels::{ChannelCodec, ChannelIncoming, platform::slack::SlackCodec};
use futures_util::{Sink, SinkExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use crate::{error::Result, socket::Session};

pub(super) async fn handle<W>(session: &Session, writer: &mut W, message: Message) -> Result<bool>
where
    W: Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
{
    match message {
        Message::Text(text) => {
            let root: Value = serde_json::from_str(&text)?;
            if let Some(envelope_id) = root.get("envelope_id").and_then(Value::as_str) {
                writer
                    .send(Message::Text(
                        json!({"envelope_id": envelope_id}).to_string().into(),
                    ))
                    .await?;
            }
            match root.get("type").and_then(Value::as_str) {
                Some("disconnect") => Ok(false),
                Some("events_api") => {
                    let payload = payload(&root)?;
                    if let Some(incoming) = SlackCodec.decode_incoming(&payload.to_string())? {
                        if let ChannelIncoming::Message(message) = &incoming
                            && let Some(reply) = session.take_input(&message.target)
                        {
                            session.command_result(
                                reply,
                                cortexfs_channels::ChannelCommandResult::Value {
                                    payload: json!({"text": message.body.text}),
                                },
                            )?;
                        } else {
                            session.send_incoming(incoming)?;
                        }
                    }
                    Ok(true)
                }
                Some("interactive") => {
                    let payload = payload(&root)?;
                    if let Some((reply, result)) = session.take_action(&payload) {
                        session.command_result(reply, result)?;
                    }
                    Ok(true)
                }
                _ => Ok(true),
            }
        }
        Message::Ping(payload) => {
            writer.send(Message::Pong(payload)).await?;
            Ok(true)
        }
        Message::Close(_) => Ok(false),
        Message::Binary(_) | Message::Pong(_) | Message::Frame(_) => Ok(true),
    }
}

fn payload(root: &Value) -> Result<Value> {
    match root.get("payload") {
        Some(Value::String(value)) => Ok(serde_json::from_str(value)?),
        Some(value) => Ok(value.clone()),
        None => Ok(root.clone()),
    }
}
