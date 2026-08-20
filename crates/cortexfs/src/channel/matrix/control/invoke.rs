use cortexfs_channels::MessageTarget;
use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::super::{MatrixConfig, MatrixError};
use super::ops::{MissingRoom, call, string, upload};

pub(super) fn run(
    client: &Client,
    config: &MatrixConfig,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
    transaction: &mut u64,
) -> Result<Value, MatrixError> {
    let room = target.map(|value| value.conversation.as_str());
    match name {
        "matrix.send_html" => send_html(
            client,
            config,
            room.ok_or(MissingRoom)?,
            payload,
            transaction,
        ),
        "matrix.upload_media" => upload(client, config, payload),
        "matrix.create_room" => call(client, config, "POST", "createRoom", payload),
        "matrix.join_room" => {
            let room = string(payload, "room_id")?;
            call(client, config, "POST", &format!("join/{room}"), &json!({}))
        }
        "matrix.invite_user" => {
            let room = room.ok_or(MissingRoom)?;
            call(
                client,
                config,
                "POST",
                &format!("rooms/{room}/invite"),
                payload,
            )
        }
        "matrix.redact_event" => {
            let room = room.ok_or(MissingRoom)?;
            let event = string(payload, "event_id")?;
            let path = format!("rooms/{room}/redact/{event}/{}", next(transaction));
            call(client, config, "PUT", &path, &json!({}))
        }
        "matrix.send_reaction" => {
            let room = room.ok_or(MissingRoom)?;
            let event = string(payload, "event_id")?;
            let emoji = string(payload, "emoji")?;
            let path = format!("rooms/{room}/send/m.reaction/{}", next(transaction));
            call(
                client,
                config,
                "PUT",
                &path,
                &json!({"m.relates_to":{"rel_type":"m.annotation","event_id":event,"key":emoji}}),
            )
        }
        "matrix.read_receipt" => {
            let room = room.ok_or(MissingRoom)?;
            let event = string(payload, "event_id")?;
            call(
                client,
                config,
                "POST",
                &format!("rooms/{room}/read_markers"),
                &json!({"m.fully_read":event,"m.read":event}),
            )
        }
        _ => Err(MatrixError::Protocol("unsupported operation".to_owned())),
    }
}

fn send_html(
    client: &Client,
    config: &MatrixConfig,
    room: &str,
    payload: &Value,
    transaction: &mut u64,
) -> Result<Value, MatrixError> {
    let html = string(payload, "html")?;
    let text = payload.get("text").and_then(Value::as_str).unwrap_or(html);
    let path = format!("rooms/{room}/send/m.room.message/{}", next(transaction));
    call(
        client,
        config,
        "PUT",
        &path,
        &json!({"msgtype":"m.text","body":text,"format":"org.matrix.custom.html","formatted_body":html}),
    )
}

fn next(transaction: &mut u64) -> String {
    *transaction = transaction.saturating_add(1);
    format!("cortexfs-tool-{transaction}")
}
