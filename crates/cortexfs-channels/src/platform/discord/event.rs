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
    let kind = root
        .get("t")
        .or_else(|| root.get("type"))
        .and_then(Value::as_str);
    let data = root.get("d").unwrap_or(&root);
    match kind {
        Some("MESSAGE_UPDATE") => edited(data, channel).map(Some),
        Some("MESSAGE_DELETE") => deleted(data, channel).map(Some),
        Some("MESSAGE_REACTION_ADD") => reaction(data, channel, true).map(Some),
        Some("MESSAGE_REACTION_REMOVE") => reaction(data, channel, false).map(Some),
        Some("TYPING_START") => typing(data, channel).map(Some),
        _ => Ok(None),
    }
}

fn edited(data: &Value, channel: ChannelId) -> Result<ChannelIncomingEvent, ChannelError> {
    Ok(ChannelIncomingEvent::MessageEdited {
        // Updates identify the message author, not the user or system that changed it.
        context: context(data, channel, None)?,
        message_id: scalar(data.get("id"), "id")?,
        body: MessageBody::text(
            data.get("content")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )?,
    })
}

fn deleted(data: &Value, channel: ChannelId) -> Result<ChannelIncomingEvent, ChannelError> {
    Ok(ChannelIncomingEvent::MessageDeleted {
        context: context(data, channel, None)?,
        message_id: scalar(data.get("id"), "id")?,
    })
}

fn reaction(
    data: &Value,
    channel: ChannelId,
    added: bool,
) -> Result<ChannelIncomingEvent, ChannelError> {
    let emoji = data
        .get("emoji")
        .and_then(|value| value.get("name").or_else(|| value.get("id")))
        .map(|value| scalar(Some(value), "emoji"))
        .transpose()?
        .ok_or_else(|| ChannelError::Protocol("discord emoji is missing".to_owned()))?;
    Ok(ChannelIncomingEvent::Reaction {
        context: context(data, channel, data.get("user_id"))?,
        message_id: scalar(data.get("message_id"), "message_id")?,
        emoji,
        added,
    })
}

fn typing(data: &Value, channel: ChannelId) -> Result<ChannelIncomingEvent, ChannelError> {
    Ok(ChannelIncomingEvent::Typing {
        context: context(data, channel, data.get("user_id"))?,
        active: true,
    })
}

fn context(
    data: &Value,
    channel: ChannelId,
    user: Option<&Value>,
) -> Result<ChannelEventContext, ChannelError> {
    let conversation = ConversationId::new(scalar(data.get("channel_id"), "channel_id")?)?;
    let participant = user
        .map(|value| scalar(Some(value), "user_id"))
        .transpose()?
        .map(|id| participant(None, id));
    Ok(ChannelEventContext {
        target: MessageTarget {
            channel,
            conversation,
            thread: data
                .get("thread_id")
                .map(|value| scalar(Some(value), "thread_id"))
                .transpose()?,
            reply_to: None,
        },
        participant,
        timestamp_ms: None,
        metadata: std::collections::BTreeMap::new(),
    })
}
