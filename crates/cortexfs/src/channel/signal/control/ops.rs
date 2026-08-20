use std::process::Command;

use cortexfs_channels::{MessageTarget, OutboundRequest};
use serde_json::{Value, json};

use super::super::SignalConfig;
use crate::channel::control::ChannelControlError;

pub(super) fn send_request(
    config: &SignalConfig,
    request: &OutboundRequest,
) -> Result<(), ChannelControlError> {
    let root: Value =
        serde_json::from_str(&request.body).map_err(|error| fail(&error.to_string()))?;
    let params = root
        .get("params")
        .ok_or_else(|| fail("signal params are missing"))?;
    let recipient = params
        .get("recipient")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .and_then(Value::as_str)
        .ok_or_else(|| fail("signal recipient is missing"))?;
    let text = params
        .get("message")
        .and_then(Value::as_str)
        .ok_or_else(|| fail("signal message is missing"))?;
    command(config, ["send", "--", recipient, "-m", text])
}

pub(super) fn run(
    config: &SignalConfig,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value, ChannelControlError> {
    let recipient = target
        .and_then(|item| item.conversation.as_str().strip_prefix("group:"))
        .or_else(|| target.map(|item| item.conversation.as_str()))
        .or_else(|| payload.get("recipient").and_then(Value::as_str))
        .ok_or_else(|| fail("recipient is missing"))?;
    match name {
        "signal.send_poll" | "signal.request_approval" => {
            let text = payload
                .get("question")
                .or_else(|| payload.get("text"))
                .and_then(Value::as_str)
                .ok_or_else(|| fail("question is missing"))?;
            command(config, ["send", "--", recipient, "-m", text])
        }
        "signal.send_attachment" => {
            let path = value(payload, "path")?;
            command(config, ["send", "--", recipient, "-a", path])
        }
        "signal.send_reaction" | "signal.remove_reaction" => {
            let message_id = value(payload, "message_id")?;
            let emoji = value(payload, "emoji")?;
            command(
                config,
                [
                    "sendReaction",
                    "--target-author",
                    recipient,
                    "--target-timestamp",
                    message_id,
                    "--emoji",
                    emoji,
                ],
            )
        }
        _ => return Err(fail("unsupported operation")),
    }?;
    Ok(json!({"accepted":true}))
}

fn command<const N: usize>(
    config: &SignalConfig,
    args: [&str; N],
) -> Result<(), ChannelControlError> {
    let status = Command::new(&config.executable)
        .arg("-a")
        .arg(&config.account)
        .args(args)
        .status()
        .map_err(|error| fail(&error.to_string()))?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| fail("signal-cli failed"))
}

fn value<'a>(payload: &'a Value, name: &'static str) -> Result<&'a str, ChannelControlError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or_else(|| fail(&format!("{name} is missing")))
}

fn fail(message: &str) -> ChannelControlError {
    ChannelControlError::Operation(message.to_owned())
}
