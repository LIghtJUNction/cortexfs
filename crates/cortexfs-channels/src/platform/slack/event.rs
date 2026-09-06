use serde_json::Value;

use super::super::{object, participant, string};
use crate::{
    ChannelError, ChannelEventContext, ChannelId, ChannelIncomingEvent, ConversationId,
    MessageBody, MessageTarget,
};

pub(super) fn decode(
    payload: &str,
    channel: ChannelId,
) -> Result<Option<ChannelIncomingEvent>, ChannelError> {
    let root = object(payload)?;
    let event = root.get("event").unwrap_or(&root);
    let kind = match event.get("type").and_then(Value::as_str) {
        Some("message") => event.get("subtype").and_then(Value::as_str),
        kind => kind,
    };
    match kind {
        Some("reaction_added") => reaction(event, channel, true).map(Some),
        Some("reaction_removed") => reaction(event, channel, false).map(Some),
        Some("message_changed") => edited(event, channel).map(Some),
        Some("message_deleted") => deleted(event, channel).map(Some),
        Some("user_typing") => typing(event, channel).map(Some),
        _ => Ok(None),
    }
}

fn reaction(
    event: &Value,
    channel: ChannelId,
    added: bool,
) -> Result<ChannelIncomingEvent, ChannelError> {
    let item = event
        .get("item")
        .ok_or_else(|| ChannelError::Protocol("slack reaction item is missing".to_owned()))?;
    let emoji = string(event.get("reaction"), "reaction")?;
    let user = event.get("user").and_then(Value::as_str);
    Ok(ChannelIncomingEvent::Reaction {
        context: context(item, None, channel, user)?,
        message_id: string(item.get("ts"), "item.ts")?,
        emoji,
        added,
    })
}

fn edited(event: &Value, channel: ChannelId) -> Result<ChannelIncomingEvent, ChannelError> {
    let message = event
        .get("message")
        .ok_or_else(|| ChannelError::Protocol("slack changed message is missing".to_owned()))?;
    let editor = message
        .get("edited")
        .and_then(|edit| edit.get("user"))
        .and_then(Value::as_str);
    Ok(ChannelIncomingEvent::MessageEdited {
        context: context(message, Some(event), channel, editor)?,
        message_id: string(message.get("ts"), "message.ts")?,
        body: MessageBody::text(
            message
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )?,
    })
}

fn deleted(event: &Value, channel: ChannelId) -> Result<ChannelIncomingEvent, ChannelError> {
    let message = event.get("previous_message").unwrap_or(event);
    Ok(ChannelIncomingEvent::MessageDeleted {
        context: context(message, Some(event), channel, None)?,
        message_id: string(
            message.get("ts").or_else(|| event.get("deleted_ts")),
            "deleted_ts",
        )?,
    })
}

fn typing(event: &Value, channel: ChannelId) -> Result<ChannelIncomingEvent, ChannelError> {
    Ok(ChannelIncomingEvent::Typing {
        context: context(
            event,
            None,
            channel,
            event.get("user").and_then(Value::as_str),
        )?,
        active: true,
    })
}

fn context(
    value: &Value,
    fallback: Option<&Value>,
    channel: ChannelId,
    user: Option<&str>,
) -> Result<ChannelEventContext, ChannelError> {
    let conversation = ConversationId::new(string(
        value
            .get("channel")
            .or_else(|| value.get("item").and_then(|item| item.get("channel")))
            .or_else(|| fallback.and_then(|root| root.get("channel"))),
        "channel",
    )?)?;
    Ok(ChannelEventContext {
        target: MessageTarget {
            channel,
            conversation,
            thread: value
                .get("thread_ts")
                .map(|value| string(Some(value), "thread_ts"))
                .transpose()?,
            reply_to: None,
        },
        participant: user.map(|id| participant(None, id.to_owned())),
        timestamp_ms: None,
        metadata: std::collections::BTreeMap::new(),
    })
}
