use std::collections::BTreeMap;

use serde_json::Value;

use super::super::{participant, string};
use crate::{ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget};

pub(super) fn decode(root: &Value) -> Result<Option<InboundMessage>, ChannelError> {
    match root.get("type").and_then(Value::as_str) {
        Some("create") => activity(root),
        Some("message") => legacy(root),
        _ => Ok(None),
    }
}

fn activity(root: &Value) -> Result<Option<InboundMessage>, ChannelError> {
    let object = root
        .get("object")
        .filter(|value| value.get("type").and_then(Value::as_str) == Some("Note"))
        .ok_or_else(|| ChannelError::Protocol("nextcloud activity object is missing".to_owned()))?;
    let conversation = string(
        root.get("target").and_then(|value| value.get("id")),
        "target.id",
    )?;
    let actor = root.get("actor").unwrap_or(&Value::Null);
    if actor.get("type").and_then(Value::as_str) == Some("Application")
        || actor
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with("bots/"))
    {
        return Ok(None);
    }
    let sender = actor_id(actor)?;
    let text = object
        .get("content")
        .and_then(Value::as_str)
        .and_then(|content| serde_json::from_str::<Value>(content).ok())
        .and_then(|content| {
            content
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .ok_or_else(|| ChannelError::Protocol("nextcloud activity text is missing".to_owned()))?;
    message(root.get("id"), conversation, sender, &text)
}

fn legacy(root: &Value) -> Result<Option<InboundMessage>, ChannelError> {
    let item = root
        .get("message")
        .ok_or_else(|| ChannelError::Protocol("nextcloud message is missing".to_owned()))?;
    if matches!(
        item.get("actorType").and_then(Value::as_str),
        Some("bots" | "application")
    ) || item
        .get("messageType")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind != "comment")
    {
        return Ok(None);
    }
    let conversation = item
        .get("token")
        .or_else(|| root.get("object").and_then(|value| value.get("token")))
        .and_then(Value::as_str)
        .ok_or_else(|| ChannelError::Protocol("nextcloud room token is missing".to_owned()))?;
    message(
        item.get("id"),
        conversation.to_owned(),
        string(item.get("actorId"), "message.actorId")?,
        &string(item.get("message"), "message.message")?,
    )
}

fn actor_id(actor: &Value) -> Result<String, ChannelError> {
    let value = string(actor.get("id"), "actor.id")?;
    Ok(value
        .strip_prefix("users/")
        .or_else(|| value.strip_prefix("bots/"))
        .unwrap_or(&value)
        .to_owned())
}

fn message(
    id: Option<&Value>,
    conversation: String,
    sender: String,
    text: &str,
) -> Result<Option<InboundMessage>, ChannelError> {
    let mut metadata = BTreeMap::new();
    metadata.insert("nextcloud.actor".to_owned(), sender.clone());
    Ok(Some(InboundMessage {
        id: id
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("nextcloud-{sender}")),
        target: MessageTarget {
            channel: ChannelId::from_static("nextcloud_talk"),
            conversation: ConversationId::new(conversation)?,
            thread: None,
            reply_to: None,
        },
        sender: participant(None, sender),
        body: MessageBody::text(text.to_owned())?,
        timestamp_ms: None,
        metadata,
    }))
}
