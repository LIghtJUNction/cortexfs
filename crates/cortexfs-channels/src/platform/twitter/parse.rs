use std::collections::BTreeMap;

use serde_json::Value;

use super::super::{participant, string};
use super::{json_request, tweet, valid_id};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget,
    OutboundMessage, OutboundRequest,
};

pub(super) fn users(root: &Value) -> BTreeMap<String, Value> {
    root.get("includes")
        .and_then(|includes| includes.get("users"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|user| Some((user.get("id")?.as_str()?.to_owned(), user.clone())))
        .collect()
}

pub(super) fn one(
    value: &Value,
    channel: ChannelId,
    users: &BTreeMap<String, Value>,
) -> Result<Option<InboundMessage>, ChannelError> {
    let id = string(value.get("id"), "twitter.id")?;
    let author_id = string(value.get("author_id"), "twitter.author_id")?;
    let text = string(value.get("text"), "twitter.text")?;
    let conversation = value
        .get("conversation_id")
        .and_then(Value::as_str)
        .unwrap_or(&id)
        .to_owned();
    let mut metadata = BTreeMap::new();
    metadata.insert("twitter.tweet_id".to_owned(), id.clone());
    metadata.insert("twitter.author_id".to_owned(), author_id.clone());
    for (key, field) in [("twitter.username", "username")] {
        if let Some(value) = users
            .get(&author_id)
            .and_then(|user| user.get(field))
            .and_then(Value::as_str)
        {
            metadata.insert(key.to_owned(), value.to_owned());
        }
    }
    if let Some(created_at) = value.get("created_at").and_then(Value::as_str) {
        metadata.insert("twitter.created_at".to_owned(), created_at.to_owned());
    }
    Ok(Some(InboundMessage {
        id: format!("twitter-{id}"),
        target: MessageTarget {
            channel,
            conversation: ConversationId::new(conversation.clone())?,
            thread: Some(conversation),
            reply_to: Some(id),
        },
        sender: participant(users.get(&author_id), author_id),
        body: MessageBody::text(text)?,
        timestamp_ms: None,
        metadata,
    }))
}

pub(super) fn outbound(message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
    message.body.validate()?;
    if !message.body.attachments.is_empty() {
        return Err(ChannelError::Unsupported(
            "twitter media attachments".to_owned(),
        ));
    }
    let text = &message.body.text;
    let dm = message.metadata.get("twitter.dm_recipient");
    if let Some(recipient) = dm {
        valid_id(recipient)?;
        if text.chars().count() > 10_000 {
            return Err(ChannelError::Unsupported(
                "twitter direct message exceeds 10000 characters".to_owned(),
            ));
        }
        return Ok(json_request(
            format!("dm_conversations/with/{recipient}/messages"),
            &tweet(text, None),
        ));
    }
    if text.chars().count() > 280 {
        return Err(ChannelError::Unsupported(
            "twitter post exceeds 280 characters".to_owned(),
        ));
    }
    let reply_to = message
        .target
        .reply_to
        .as_deref()
        .or_else(|| message.metadata.get("twitter.tweet_id").map(String::as_str));
    reply_to.into_iter().try_for_each(valid_id)?;
    Ok(json_request("tweets".to_owned(), &tweet(text, reply_to)))
}
