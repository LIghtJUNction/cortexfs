use std::{collections::BTreeMap, time::Duration};

use serde_json::Value;

use super::{
    super::{participant, string},
    form,
};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget,
    OutboundMessage, OutboundRequest,
};
pub(super) fn one(
    value: &Value,
    channel: ChannelId,
) -> Result<Option<InboundMessage>, ChannelError> {
    let item = value.get("data").unwrap_or(value);
    if item.get("new").and_then(Value::as_bool) == Some(false) {
        return Ok(None);
    }
    let name = string(item.get("name"), "reddit.name")?;
    let author = string(item.get("author"), "reddit.author")?;
    let Some(body) = item
        .get("body")
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
    else {
        return Ok(None);
    };
    let parent = item
        .get("parent_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let link = item
        .get("link_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let comment = parent.is_some()
        || link.is_some()
        || item.get("type").and_then(Value::as_str) == Some("comment_reply");
    let conversation = if comment {
        parent
            .as_deref()
            .or(link.as_deref())
            .unwrap_or(name.as_str())
            .to_owned()
    } else {
        author.clone()
    };
    let mut metadata = BTreeMap::new();
    metadata.insert("reddit.name".to_owned(), name.clone());
    metadata.insert(
        "reddit.kind".to_owned(),
        if comment { "comment" } else { "dm" }.to_owned(),
    );
    for (key, field) in [
        ("reddit.subreddit", "subreddit"),
        ("reddit.subject", "subject"),
    ] {
        if let Some(value) = item
            .get(field)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
        {
            metadata.insert(key.to_owned(), value.to_owned());
        }
    }
    Ok(Some(InboundMessage {
        id: format!("reddit-{name}"),
        target: MessageTarget {
            channel,
            conversation: ConversationId::new(conversation)?,
            thread: parent,
            reply_to: Some(name),
        },
        sender: participant(None, author),
        body: MessageBody::text(body)?,
        timestamp_ms: timestamp(item.get("created_utc")),
        metadata,
    }))
}
pub(super) fn outbound(message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
    message.body.validate()?;
    if !message.body.attachments.is_empty() {
        return Err(ChannelError::Unsupported("reddit attachments".to_owned()));
    }
    let kind = message.metadata.get("reddit.kind").map(String::as_str);
    let target = message
        .metadata
        .get("reddit.name")
        .or(message.target.reply_to.as_ref())
        .map_or(message.target.conversation.as_str(), String::as_str);
    if kind == Some("comment") || is_thing(target) {
        return Ok(form::request(
            "api/comment",
            [("thing_id", target), ("text", message.body.text.as_str())],
        ));
    }
    let subject = message
        .metadata
        .get("reddit.subject")
        .map_or("CortexFS reply", String::as_str);
    Ok(form::request(
        "api/compose",
        [
            ("to", target),
            ("subject", subject),
            ("text", message.body.text.as_str()),
        ],
    ))
}
fn is_thing(value: &str) -> bool {
    value.starts_with("t1_") || value.starts_with("t3_") || value.starts_with("t4_")
}
fn timestamp(value: Option<&Value>) -> Option<u64> {
    let seconds = value.and_then(Value::as_f64)?;
    let duration = Duration::try_from_secs_f64(seconds).ok()?;
    u64::try_from(duration.as_millis()).ok()
}
