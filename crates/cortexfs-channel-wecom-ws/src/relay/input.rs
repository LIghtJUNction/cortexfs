use std::collections::BTreeMap;

use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::{
    config::Config,
    error::{Error, Result},
    message::{self, InboundEvent},
    output, socket,
};

pub(super) async fn receive(
    result: std::result::Result<Message, tokio_tungstenite::tungstenite::Error>,
    config: &Config,
    session: &socket::Session,
    output_tx: &mpsc::Sender<Message>,
    pending: &mut BTreeMap<String, InboundEvent>,
) -> Result<bool> {
    match result? {
        Message::Text(text) => {
            let frame: Value = serde_json::from_str(&text)?;
            if output::reconnect(&frame) {
                return Ok(false);
            }
            if let Some(event) = message::decode(&frame, config)? {
                session.send(event.message.clone())?;
                pending.insert(event.message.id.clone(), event);
            } else if is_enter_chat(&frame)
                && let Some(request_id) = message::request_id(&frame)
            {
                send(output_tx, output::welcome(request_id)).await?;
            }
        }
        Message::Ping(value) => output_tx
            .send(Message::Pong(value))
            .await
            .map_err(|_error| Error::Protocol("WeCom output queue closed".to_owned()))?,
        Message::Close(_) => return Ok(false),
        Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => {}
    }
    Ok(true)
}

fn is_enter_chat(frame: &Value) -> bool {
    frame.get("cmd").and_then(Value::as_str) == Some("aibot_event_callback")
        && frame
            .get("body")
            .and_then(|body| body.get("event"))
            .and_then(|event| event.get("eventtype"))
            .and_then(Value::as_str)
            == Some("enter_chat")
}

async fn send(output_tx: &mpsc::Sender<Message>, text: String) -> Result<()> {
    output_tx
        .send(Message::Text(text.into()))
        .await
        .map_err(|_error| Error::Protocol("WeCom output queue closed".to_owned()))
}
