use cortexfs_channels::{ChannelCodec, ChannelIncoming, platform::matrix::MatrixCodec};
use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::{MatrixConfig, MatrixError, api};
use crate::channel::bridge::AgentChannelBridge;

pub(super) fn run_once(
    client: &Client,
    config: &MatrixConfig,
    bridge: &AgentChannelBridge,
    user_id: &str,
    since: &mut Option<String>,
    transaction: &mut u64,
) -> Result<(), MatrixError> {
    let root = api::sync(client, config, since.as_deref())?;
    let next = root
        .get("next_batch")
        .and_then(Value::as_str)
        .ok_or_else(|| MatrixError::Protocol("sync next_batch is missing".to_owned()))?;
    let rooms = root.pointer("/rooms/join").and_then(Value::as_object);
    if let Some(rooms) = rooms {
        for (room_id, room) in rooms {
            if !config.rooms.is_empty() && !config.rooms.iter().any(|room| room == room_id) {
                continue;
            }
            let events = room.pointer("/timeline/events").and_then(Value::as_array);
            for event in events.into_iter().flatten() {
                let Some(incoming) = decode(room_id, event, user_id)? else {
                    continue;
                };
                let outbound = match incoming {
                    ChannelIncoming::Message(message) => bridge.handle(message),
                    ChannelIncoming::Event(event) => bridge.handle_event(&event),
                };
                let Ok(outbound) = outbound else {
                    continue;
                };
                let request = MatrixCodec.encode(&outbound)?;
                *transaction = transaction.saturating_add(1);
                api::send(client, config, request, &format!("cortexfs-{transaction}"))?;
            }
        }
    }
    *since = Some(next.to_owned());
    Ok(())
}

fn decode(
    room_id: &str,
    event: &Value,
    user_id: &str,
) -> Result<Option<ChannelIncoming>, MatrixError> {
    if event.get("sender").and_then(Value::as_str) == Some(user_id) {
        return Ok(None);
    }
    let mut event = event.clone();
    let Some(event) = event.as_object_mut() else {
        return Ok(None);
    };
    event.insert("room_id".to_owned(), json!(room_id));
    let payload = Value::Object(event.clone()).to_string();
    Ok(MatrixCodec.decode_incoming(&payload)?)
}
