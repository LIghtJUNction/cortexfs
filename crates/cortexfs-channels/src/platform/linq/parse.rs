use std::collections::BTreeMap;

use serde_json::Value;

use super::super::{participant, scalar, string};
use crate::{
    Attachment, ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget,
};

pub(super) fn decode(
    root: &Value,
    channel: ChannelId,
) -> Result<Option<InboundMessage>, ChannelError> {
    let event = string(root.get("event_type"), "event_type")?;
    if event != "message.received" {
        return Ok(None);
    }
    let data = object(root.get("data"), "data")?;
    let from = data
        .get("from")
        .and_then(Value::as_str)
        .or_else(|| {
            data.get("sender_handle")
                .and_then(|v| v.get("handle"))
                .and_then(Value::as_str)
        })
        .ok_or_else(|| ChannelError::Protocol("linq sender is missing".to_owned()))?;
    if data.get("is_from_me").and_then(Value::as_bool) == Some(true)
        || data
            .get("sender_handle")
            .and_then(|v| v.get("is_me"))
            .and_then(Value::as_bool)
            == Some(true)
        || data.get("direction").and_then(Value::as_str) == Some("outbound")
    {
        return Ok(None);
    }
    let sender = if from.starts_with('+') {
        from.to_owned()
    } else {
        format!("+{from}")
    };
    let parts = data
        .get("message")
        .and_then(|v| v.get("parts"))
        .or_else(|| data.get("parts"))
        .and_then(Value::as_array)
        .ok_or_else(|| ChannelError::Protocol("linq message parts are missing".to_owned()))?;
    let mut text = String::new();
    let mut attachments = Vec::new();
    for part in parts {
        match part.get("type").and_then(Value::as_str) {
            Some("text") => text.push_str(
                part.get("value")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
            ),
            Some("media" | "image") => {
                if let Some(url) = part
                    .get("url")
                    .or_else(|| part.get("value"))
                    .and_then(Value::as_str)
                {
                    attachments.push(Attachment {
                        url: url.to_owned(),
                        name: part.get("name").and_then(Value::as_str).map(str::to_owned),
                        mime: part
                            .get("mime_type")
                            .and_then(Value::as_str)
                            .map(str::to_owned),
                    });
                }
            }
            _ => {}
        }
    }
    let conversation = data
        .get("chat_id")
        .or_else(|| data.get("chat").and_then(|v| v.get("id")))
        .and_then(Value::as_str)
        .unwrap_or(&sender)
        .to_owned();
    let id = data
        .get("message")
        .and_then(|v| v.get("id"))
        .or_else(|| data.get("id"))
        .or_else(|| root.get("event_id"));
    let mut metadata = BTreeMap::new();
    if let Some(value) = root.get("created_at").and_then(Value::as_str) {
        metadata.insert("linq.created_at".to_owned(), value.to_owned());
    }
    Ok(Some(InboundMessage {
        id: scalar(id, "linq message id")?,
        target: MessageTarget {
            channel,
            conversation: ConversationId::new(conversation)?,
            thread: None,
            reply_to: None,
        },
        sender: participant(None, sender),
        body: MessageBody::with_attachments(text.trim(), attachments)?,
        timestamp_ms: None,
        metadata,
    }))
}

fn object<'a>(value: Option<&'a Value>, field: &str) -> Result<&'a Value, ChannelError> {
    value
        .filter(|value| value.is_object())
        .ok_or_else(|| ChannelError::Protocol(format!("linq {field} is missing")))
}
