#![expect(
    clippy::field_scoped_visibility_modifiers,
    clippy::redundant_pub_crate,
    reason = "message conversion is private driver plumbing"
)]

use std::collections::BTreeMap;

use serde_json::Value;

use crate::{
    config::Config,
    error::{Error, Result},
};

pub(crate) struct InboundEvent {
    pub(crate) request_id: String,
    pub(crate) message: cortexfs_channels::InboundMessage,
}

pub(crate) fn decode(frame: &Value, config: &Config) -> Result<Option<InboundEvent>> {
    if frame.get("cmd").and_then(Value::as_str) != Some("aibot_msg_callback") {
        return Ok(None);
    }
    let request_id = string(frame, &["headers", "req_id"])?;
    let body = frame
        .get("body")
        .ok_or_else(|| Error::Protocol("WeCom callback body is missing".to_owned()))?;
    if body.get("msgtype").and_then(Value::as_str) != Some("text") {
        return Ok(None);
    }
    let sender = string(body, &["from", "userid"])?;
    let chat_type = body
        .get("chattype")
        .and_then(Value::as_str)
        .unwrap_or("single");
    let group = body.get("chatid").and_then(Value::as_str);
    if !config.allowed(
        &sender,
        group.filter(|_| chat_type.eq_ignore_ascii_case("group")),
    ) {
        return Ok(None);
    }
    let text = string(body, &["text", "content"])?;
    let conversation = group
        .filter(|_| chat_type.eq_ignore_ascii_case("group"))
        .map_or_else(
            || format!("user:{sender}"),
            |value| format!("group:{value}"),
        );
    let id = body
        .get("msgid")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(&request_id)
        .to_owned();
    let mut metadata = BTreeMap::new();
    metadata.insert("wecom_req_id".to_owned(), request_id.clone());
    metadata.insert("wecom_chat_type".to_owned(), chat_type.to_owned());
    Ok(Some(InboundEvent {
        request_id,
        message: cortexfs_channels::InboundMessage {
            id,
            target: cortexfs_channels::MessageTarget {
                channel: cortexfs_channels::ChannelId::from_static("wecom-ws"),
                conversation: cortexfs_channels::ConversationId::new(conversation)?,
                thread: None,
                reply_to: None,
            },
            sender: cortexfs_channels::Participant {
                id: sender,
                ..Default::default()
            },
            body: cortexfs_channels::MessageBody::text(text)?,
            timestamp_ms: None,
            metadata,
        },
    }))
}

fn string(root: &Value, path: &[&str]) -> Result<String> {
    path.iter()
        .try_fold(root, |value, key| value.get(*key))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| Error::Protocol(format!("WeCom field {} is missing", path.join("."))))
}

pub(crate) fn request_id(frame: &Value) -> Option<&str> {
    frame.get("headers")?.get("req_id")?.as_str()
}

#[cfg(test)]
mod tests;
