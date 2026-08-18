use serde_json::{Value, json};

use super::super::{participant, string};
use crate::{ChannelError, ChannelId, InboundMessage, MessageBody, MessageTarget, OutboundMessage};

pub(super) fn one(
    value: &Value,
    channel: ChannelId,
) -> Result<Option<InboundMessage>, ChannelError> {
    if !matches!(
        value.get("reason").and_then(Value::as_str),
        Some("mention" | "reply")
    ) || value.get("isRead").and_then(Value::as_bool) == Some(true)
    {
        return Ok(None);
    }
    let author = value
        .get("author")
        .ok_or_else(|| ChannelError::Protocol("bluesky author is missing".to_owned()))?;
    let sender = string(author.get("did"), "author.did")?;
    let uri = string(value.get("uri"), "notification.uri")?;
    let cid = string(value.get("cid"), "notification.cid")?;
    let record = value.get("record").and_then(|record| {
        record
            .get("text")
            .or_else(|| record.get("value").and_then(|value| value.get("text")))
    });
    let body = MessageBody::text(string(record, "record.text")?)?;
    let mut metadata = std::collections::BTreeMap::new();
    metadata.insert("bluesky.reply_uri".to_owned(), uri.clone());
    metadata.insert("bluesky.reply_cid".to_owned(), cid.clone());
    if let Some(seen_at) = value.get("indexedAt").and_then(Value::as_str) {
        metadata.insert("bluesky.seen_at".to_owned(), seen_at.to_owned());
    }
    if let Some(handle) = author.get("handle").and_then(Value::as_str) {
        metadata.insert("bluesky.handle".to_owned(), handle.to_owned());
    }
    Ok(Some(InboundMessage {
        id: format!("bluesky-{cid}"),
        target: MessageTarget {
            channel,
            conversation: crate::ConversationId::new(sender.clone())?,
            thread: Some(uri.clone()),
            reply_to: Some(format!("{uri}|{cid}")),
        },
        sender: participant(Some(author), sender),
        body,
        timestamp_ms: None,
        metadata,
    }))
}

pub(super) fn reply(message: &OutboundMessage) -> Value {
    let uri = message
        .metadata
        .get("bluesky.reply_uri")
        .map(String::as_str)
        .or(message.target.reply_to.as_deref())
        .and_then(|value| value.split('|').next());
    let cid = message
        .metadata
        .get("bluesky.reply_cid")
        .map(String::as_str)
        .or_else(|| {
            message
                .target
                .reply_to
                .as_deref()
                .and_then(|value| value.split('|').nth(1))
        });
    match (uri, cid) {
        (Some(uri), Some(cid)) if !uri.is_empty() && !cid.is_empty() => {
            json!({"root":{"uri":uri,"cid":cid},"parent":{"uri":uri,"cid":cid}})
        }
        _ => Value::Null,
    }
}
