use serde_json::Value;

use super::super::{object, participant, scalar, text, timestamp_ms};
use crate::{
    ChannelError, ChannelEventContext, ChannelId, ChannelIncomingEvent, ConversationId,
    MessageTarget,
};

pub(super) fn decode(
    payload: &str,
    channel: ChannelId,
) -> Result<Option<ChannelIncomingEvent>, ChannelError> {
    let root = object(payload)?;
    if let Some(reaction) = root.get("message_reaction") {
        return decode_reaction(reaction, channel).map(Some);
    }
    let Some(message) = root
        .get("edited_message")
        .or_else(|| root.get("edited_channel_post"))
    else {
        return Ok(None);
    };
    let context = context(message, channel)?;
    let message_id = scalar(message.get("message_id"), "message_id")?;
    let body = text(message.get("text").or_else(|| message.get("caption")))?;
    Ok(Some(ChannelIncomingEvent::MessageEdited {
        context,
        message_id,
        body,
    }))
}

fn decode_reaction(
    reaction: &Value,
    channel: ChannelId,
) -> Result<ChannelIncomingEvent, ChannelError> {
    let context = context_from_reaction(reaction, channel)?;
    let message_id = scalar(reaction.get("message_id"), "message_id")?;
    let new = reaction
        .get("new_reaction")
        .and_then(Value::as_array)
        .and_then(|items| items.first());
    let old = reaction
        .get("old_reaction")
        .and_then(Value::as_array)
        .and_then(|items| items.first());
    let selected = new
        .or(old)
        .ok_or_else(|| ChannelError::Protocol("telegram reaction has no emoji value".to_owned()))?;
    let emoji = selected
        .get("emoji")
        .or_else(|| selected.get("custom_emoji_id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ChannelError::Protocol("telegram reaction emoji is missing".to_owned()))?;
    Ok(ChannelIncomingEvent::Reaction {
        context,
        message_id,
        emoji,
        added: new.is_some(),
    })
}

fn context(message: &Value, channel: ChannelId) -> Result<ChannelEventContext, ChannelError> {
    let chat = message
        .get("chat")
        .ok_or_else(|| ChannelError::Protocol("telegram chat is missing".to_owned()))?;
    let conversation = ConversationId::new(scalar(chat.get("id"), "chat.id")?)?;
    Ok(ChannelEventContext {
        target: MessageTarget {
            channel,
            conversation,
            thread: message
                .get("message_thread_id")
                .map(|value| scalar(Some(value), "message_thread_id"))
                .transpose()?,
            reply_to: None,
        },
        participant: message
            .get("from")
            .map(|from| scalar(from.get("id"), "from.id"))
            .transpose()?
            .map(|id| participant(message.get("from"), id)),
        timestamp_ms: timestamp_ms(message.get("date")),
        metadata: std::collections::BTreeMap::new(),
    })
}

fn context_from_reaction(
    reaction: &Value,
    channel: ChannelId,
) -> Result<ChannelEventContext, ChannelError> {
    let chat = reaction
        .get("chat")
        .ok_or_else(|| ChannelError::Protocol("telegram reaction chat is missing".to_owned()))?;
    let conversation = ConversationId::new(scalar(chat.get("id"), "chat.id")?)?;
    Ok(ChannelEventContext {
        target: MessageTarget {
            channel,
            conversation,
            thread: None,
            reply_to: None,
        },
        participant: reaction
            .get("user")
            .map(|user| scalar(user.get("id"), "user.id"))
            .transpose()?
            .map(|id| participant(reaction.get("user"), id)),
        timestamp_ms: None,
        metadata: std::collections::BTreeMap::new(),
    })
}
