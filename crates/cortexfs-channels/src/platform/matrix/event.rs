use serde_json::Value;

use super::super::{object, participant, scalar};
use crate::{
    ChannelError, ChannelEventContext, ChannelId, ChannelIncomingEvent, ConversationId,
    MessageBody, MessageTarget,
};

pub(super) fn decode(
    payload: &str,
    channel: ChannelId,
) -> Result<Option<ChannelIncomingEvent>, ChannelError> {
    let root = object(payload)?;
    let event_type = root.get("type").and_then(Value::as_str);
    match event_type {
        Some("m.reaction") => reaction(&root, channel).map(Some),
        Some("m.room.redaction") => redaction(&root, channel).map(Some),
        Some("m.room.message") if replacement(&root) => edited(&root, channel).map(Some),
        _ => Ok(None),
    }
}

fn reaction(root: &Value, channel: ChannelId) -> Result<ChannelIncomingEvent, ChannelError> {
    let relation = root
        .pointer("/content/m.relates_to")
        .ok_or_else(|| ChannelError::Protocol("matrix reaction relation is missing".to_owned()))?;
    if relation.get("rel_type").and_then(Value::as_str) != Some("m.annotation") {
        return Err(ChannelError::Protocol(
            "matrix reaction relation is invalid".to_owned(),
        ));
    }
    Ok(ChannelIncomingEvent::Reaction {
        context: context(root, channel)?,
        message_id: scalar(relation.get("event_id"), "m.relates_to.event_id")?,
        emoji: scalar(relation.get("key"), "m.relates_to.key")?,
        added: true,
    })
}

fn redaction(root: &Value, channel: ChannelId) -> Result<ChannelIncomingEvent, ChannelError> {
    Ok(ChannelIncomingEvent::MessageDeleted {
        context: context(root, channel)?,
        message_id: scalar(root.get("redacts"), "redacts")?,
    })
}

fn edited(root: &Value, channel: ChannelId) -> Result<ChannelIncomingEvent, ChannelError> {
    let content = root
        .get("content")
        .and_then(|value| value.get("m.new_content"))
        .ok_or_else(|| {
            ChannelError::Protocol("matrix replacement content is missing".to_owned())
        })?;
    Ok(ChannelIncomingEvent::MessageEdited {
        context: context(root, channel)?,
        message_id: scalar(
            root.pointer("/content/m.relates_to/event_id"),
            "m.relates_to.event_id",
        )?,
        body: MessageBody::text(scalar(content.get("body"), "m.new_content.body")?)?,
    })
}

fn replacement(root: &Value) -> bool {
    root.pointer("/content/m.relates_to/rel_type")
        .and_then(Value::as_str)
        == Some("m.replace")
}

fn context(root: &Value, channel: ChannelId) -> Result<ChannelEventContext, ChannelError> {
    let conversation = ConversationId::new(scalar(root.get("room_id"), "room_id")?)?;
    Ok(ChannelEventContext {
        target: MessageTarget {
            channel,
            conversation,
            thread: None,
            reply_to: None,
        },
        participant: root
            .get("sender")
            .map(|value| scalar(Some(value), "sender").map(|id| participant(None, id)))
            .transpose()?,
        timestamp_ms: root.get("origin_server_ts").and_then(Value::as_u64),
        metadata: std::collections::BTreeMap::new(),
    })
}
