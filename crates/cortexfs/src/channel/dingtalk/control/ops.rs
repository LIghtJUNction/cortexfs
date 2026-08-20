use cortexfs_channels::{
    ChannelCodec, MessageBody, MessageTarget, OutboundMessage, OutboundRequest,
    platform::dingtalk::DingTalkCodec,
};
use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::super::{DingTalkConfig, Webhooks, api};
use crate::channel::control::ChannelControlError;

use super::sign;

pub(super) fn run(
    client: &Client,
    config: &DingTalkConfig,
    webhooks: &Webhooks,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value, ChannelControlError> {
    if name == "dingtalk.sign_request" {
        return Ok(json!({
            "timestamp": value(payload, "timestamp")?,
            "signature": sign::run(config, payload)?
        }));
    }
    let target = target.ok_or_else(|| fail("target is missing"))?;
    let request = match name {
        "dingtalk.send_markdown" => DingTalkCodec
            .encode(&OutboundMessage {
                target: target.clone(),
                body: MessageBody::text(value(payload, "text")?)
                    .map_err(|error| fail(&error.to_string()))?,
                metadata: std::collections::BTreeMap::new(),
            })
            .map_err(|error| fail(&error.to_string()))?,
        "dingtalk.send_action_card" => request(
            target,
            &json!({
                "msgtype":"actionCard",
                "actionCard":payload,
            }),
        ),
        "dingtalk.send_image" => request(
            target,
            &json!({
                "msgtype":"image",
                "image":payload,
            }),
        ),
        "dingtalk.send_file" => request(
            target,
            &json!({
                "msgtype":"file",
                "file":payload,
            }),
        ),
        _ => return Err(fail("unsupported operation")),
    };
    send(client, webhooks, request)?;
    Ok(json!({"accepted":true}))
}

pub(super) fn send(
    client: &Client,
    webhooks: &Webhooks,
    request: OutboundRequest,
) -> Result<(), ChannelControlError> {
    let conversation = request
        .headers
        .get("DingTalk-Conversation")
        .ok_or_else(|| fail("conversation is missing"))?;
    let webhook = webhooks
        .lock()
        .ok()
        .and_then(|items| items.get(conversation).cloned())
        .ok_or_else(|| fail("DingTalk session webhook is not known"))?;
    api::reply(client, &webhook, request).map_err(|error| fail(&error.to_string()))
}

fn request(target: &MessageTarget, body: &Value) -> OutboundRequest {
    OutboundRequest {
        method: "POST".to_owned(),
        path: "sessionWebhook".to_owned(),
        content_type: "application/json".to_owned(),
        body: body.to_string(),
        headers: std::collections::BTreeMap::from([(
            "DingTalk-Conversation".to_owned(),
            target.conversation.to_string(),
        )]),
    }
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
