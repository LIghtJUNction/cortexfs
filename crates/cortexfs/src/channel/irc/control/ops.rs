use cortexfs_channels::MessageTarget;
use serde_json::Value;

use crate::channel::control::ChannelControlError;

pub(super) fn line(
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<String, ChannelControlError> {
    let destination = target
        .map(|item| item.conversation.as_str())
        .or_else(|| payload.get("channel").and_then(Value::as_str))
        .ok_or_else(|| fail("channel is missing"))?;
    let line = match name {
        "irc.raw" => value(payload, "line")?.to_owned(),
        "irc.join" => format!("JOIN {destination}\r\n"),
        "irc.part" => format!("PART {destination}\r\n"),
        "irc.notice" => format!("NOTICE {destination} :{}\r\n", value(payload, "text")?),
        "irc.action" => format!(
            "PRIVMSG {destination} :\x01ACTION {}\x01\r\n",
            value(payload, "text")?
        ),
        "irc.topic" => format!("TOPIC {destination} :{}\r\n", value(payload, "topic")?),
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
