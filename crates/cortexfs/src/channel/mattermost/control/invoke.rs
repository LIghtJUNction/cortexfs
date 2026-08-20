use cortexfs_channels::{
    ChannelCodec, MessageBody, MessageTarget, OutboundMessage,
    platform::mattermost::MattermostCodec,
};
use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::super::{MattermostConfig, MattermostError, api};
use super::upload;

pub(super) fn run(
    client: &Client,
    config: &MattermostConfig,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value, MattermostError> {
    let Some(target) = target else {
        return Err(MattermostError::Protocol("target is missing".to_owned()));
    };
    match name {
        "mattermost.post" => send(client, config, target, payload),
        "mattermost.thread_reply" => {
            let mut target = target.clone();
            target.thread = Some(string(payload, "root_id")?.to_owned());
            send(client, config, &target, payload)
        }
        "mattermost.add_reaction" | "mattermost.pin_post" => {
            effect(client, config, target, payload, false)
        }
        "mattermost.remove_reaction" | "mattermost.unpin_post" => {
            effect(client, config, target, payload, true)
        }
        "mattermost.upload" => upload::run(client, config, target, payload),
        _ => Err(MattermostError::Protocol(
            "unsupported operation".to_owned(),
        )),
    }
}

fn send(
    client: &Client,
    config: &MattermostConfig,
    target: &MessageTarget,
    payload: &Value,
) -> Result<Value, MattermostError> {
    let text = string(payload, "text")?;
    let message = OutboundMessage {
        target: target.clone(),
        body: MessageBody::text(text.to_owned())?,
        metadata: std::collections::BTreeMap::new(),
    };
    api::send(client, config, MattermostCodec.encode(&message)?)?;
    Ok(json!({"accepted":true}))
}

fn effect(
    client: &Client,
    config: &MattermostConfig,
    target: &MessageTarget,
    payload: &Value,
    remove: bool,
) -> Result<Value, MattermostError> {
    let message_id = string(payload, "message_id")?;
    let effect = if payload.get("emoji").is_some() {
        cortexfs_channels::ChannelEffect::Reaction {
            message_id: message_id.to_owned(),
            emoji: string(payload, "emoji")?.to_owned(),
            remove,
        }
    } else if remove {
        cortexfs_channels::ChannelEffect::Unpin {
            message_id: message_id.to_owned(),
        }
    } else {
        cortexfs_channels::ChannelEffect::Pin {
            message_id: message_id.to_owned(),
        }
    };
    if let Some(request) = MattermostCodec.encode_effect(target, &effect)? {
        api::send(client, config, request)?;
    }
    Ok(json!({"accepted":true}))
}

fn string<'a>(value: &'a Value, name: &'static str) -> Result<&'a str, MattermostError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or(MattermostError::Protocol(format!("{name} is missing")))
}
