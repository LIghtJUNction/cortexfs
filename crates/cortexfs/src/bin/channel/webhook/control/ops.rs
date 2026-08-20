use std::collections::BTreeMap;

use cortexfs_channels::{ChannelCodec, MessageBody, MessageTarget, OutboundMessage};
use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::super::{WebhookConfig, outbound};
use cortexfs::channel::control::ChannelControlError;

use super::native;

pub(super) fn run(
    client: &Client,
    config: &WebhookConfig,
    codec: &dyn ChannelCodec,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value, ChannelControlError> {
    if name == "webhook.health" {
        return Ok(json!({"status":"ready"}));
    }
    if name == "webhook.challenge" {
        let challenge = payload
            .get("challenge")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && !value.contains('\0'))
            .ok_or_else(|| fail("challenge is missing"))?;
        return Ok(json!({"challenge":challenge}));
    }
    let target = target.ok_or_else(|| fail("target is missing"))?;
    let request = if name == "webhook.send" {
        let message = OutboundMessage {
            target: target.clone(),
            body: MessageBody::text(value(payload, "text")?)
                .map_err(|error| fail(&error.to_string()))?,
            metadata: BTreeMap::new(),
        };
        codec
            .encode(&message)
            .map_err(|error| fail(&error.to_string()))?
    } else {
        native::request(config.platform, target, name, payload)?
    };
    outbound::send(client, config, request).map_err(|error| fail(&error.to_string()))?;
    Ok(json!({"accepted":true}))
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
