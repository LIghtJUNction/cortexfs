use std::time::Duration;

use serde_json::Value;

use super::DiscordError;

pub(super) enum GatewayEvent {
    Hello(Duration),
    Dispatch {
        name: String,
        data: Value,
        sequence: Option<i64>,
    },
    Heartbeat,
    Reconnect,
    InvalidSession,
    Ignore,
}

pub(super) fn parse(payload: &str) -> Result<GatewayEvent, DiscordError> {
    let root: Value = serde_json::from_str(payload)?;
    let opcode = root
        .get("op")
        .and_then(Value::as_i64)
        .ok_or_else(|| DiscordError::Protocol("gateway opcode is missing".to_owned()))?;
    match opcode {
        0 => Ok(GatewayEvent::Dispatch {
            name: root
                .get("t")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            data: root.get("d").cloned().unwrap_or(Value::Null),
            sequence: root.get("s").and_then(Value::as_i64),
        }),
        1 => Ok(GatewayEvent::Heartbeat),
        7 => Ok(GatewayEvent::Reconnect),
        9 => Ok(GatewayEvent::InvalidSession),
        10 => {
            let milliseconds = root
                .get("d")
                .and_then(|value| value.get("heartbeat_interval"))
                .and_then(Value::as_u64)
                .ok_or_else(|| {
                    DiscordError::Protocol("heartbeat interval is missing".to_owned())
                })?;
            Ok(GatewayEvent::Hello(Duration::from_millis(milliseconds)))
        }
        _ => Ok(GatewayEvent::Ignore),
    }
}
