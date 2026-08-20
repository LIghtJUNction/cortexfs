use cortexfs_channels::MessageTarget;
use serde_json::Value;

use crate::channel::control::ChannelControlError;

pub(super) fn line(
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<String, ChannelControlError> {
    let channel = target
        .map(|item| item.conversation.as_str())
        .or_else(|| payload.get("channel").and_then(Value::as_str))
        .ok_or_else(|| fail("channel is missing"))?;
    let line = match name {
        "twitch.send_whisper" => format!(
            "PRIVMSG {channel} :/w {} {}\r\n",
            value(payload, "user")?,
            value(payload, "text")?
        ),
        "twitch.set_title" => format!("PRIVMSG {channel} :/title {}\r\n", value(payload, "title")?),
        "twitch.set_game" => format!("PRIVMSG {channel} :/game {}\r\n", value(payload, "game")?),
        "twitch.timeout" => format!(
            "PRIVMSG {channel} :/timeout {} {} {}\r\n",
            value(payload, "user")?,
            value(payload, "seconds")?,
            payload.get("reason").and_then(Value::as_str).unwrap_or("")
        ),
        "twitch.ban" => format!(
            "PRIVMSG {channel} :/ban {} {}\r\n",
            value(payload, "user")?,
            payload.get("reason").and_then(Value::as_str).unwrap_or("")
        ),
        _ => return Err(fail("unsupported operation")),
    };
    Ok(line)
}

fn value<'a>(payload: &'a Value, name: &'static str) -> Result<&'a str, ChannelControlError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains(['\0', '\r', '\n']))
        .ok_or_else(|| fail(&format!("{name} is missing")))
}

fn fail(message: &str) -> ChannelControlError {
    ChannelControlError::Operation(message.to_owned())
}
