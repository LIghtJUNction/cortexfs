use std::collections::BTreeMap;

use cortexfs_channels::{
    ChannelCodec, MessageBody, MessageTarget, OutboundMessage, platform::email::EmailCodec,
};
use serde_json::{Value, json};

use super::super::{EmailConfig, attachment, imap, smtp};
use crate::channel::control::ChannelControlError;

pub(super) fn run(
    config: &EmailConfig,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value, ChannelControlError> {
    if matches!(name, "email.search" | "email.read" | "email.mark_read") {
        return imap::tool(config, name, payload).map_err(|error| fail(error.to_string()));
    }
    if name == "email.send_attachment" {
        return send_attachment(config, target, payload);
    }
    if !matches!(name, "email.reply" | "email.forward") {
        return Err(fail("unsupported operation".to_owned()));
    }
    let target = target
        .cloned()
        .ok_or_else(|| fail("target is missing".to_owned()))?;
    let text = value(payload, "text")?;
    let mut metadata = BTreeMap::new();
    metadata.insert(
        "email.from".to_owned(),
        payload
            .get("to")
            .and_then(Value::as_str)
            .unwrap_or(target.conversation.as_str())
            .to_owned(),
    );
    metadata.insert(
        "email.subject".to_owned(),
        payload
            .get("subject")
            .and_then(Value::as_str)
            .unwrap_or(if name == "email.forward" {
                "Fwd: CortexFS"
            } else {
                "Re: CortexFS"
            })
            .to_owned(),
    );
    let message = OutboundMessage {
        target,
        body: MessageBody::text(text).map_err(|error| fail(error.to_string()))?,
        metadata,
    };
    let request = EmailCodec
        .encode(&message)
        .map_err(|error| fail(error.to_string()))?;
    smtp::send_request(config, &request).map_err(|error| fail(error.to_string()))?;
    Ok(json!({"accepted":true}))
}

fn send_attachment(
    config: &EmailConfig,
    target: Option<&MessageTarget>,
    payload: &Value,
) -> Result<Value, ChannelControlError> {
    let recipient = payload
        .get("to")
        .and_then(Value::as_str)
        .or_else(|| target.map(|item| item.conversation.as_str()))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| fail("recipient is missing".to_owned()))?;
    let name = value(payload, "name")?;
    let encoded = value(payload, "data_base64")?;
    let subject = payload
        .get("subject")
        .and_then(Value::as_str)
        .unwrap_or("CortexFS attachment");
    let text = payload.get("text").and_then(Value::as_str).unwrap_or("");
    let mime = payload
        .get("mime")
        .and_then(Value::as_str)
        .unwrap_or("application/octet-stream");
    attachment::send(
        config,
        &attachment::Request {
            recipient,
            subject,
            text,
            name,
            mime,
            encoded,
        },
    )
    .map_err(|error| fail(error.to_string()))?;
    Ok(json!({"accepted":true}))
}

fn value<'a>(payload: &'a Value, name: &'static str) -> Result<&'a str, ChannelControlError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or_else(|| fail(format!("{name} is missing")))
}

fn fail(error: String) -> ChannelControlError {
    ChannelControlError::Operation(error)
}
