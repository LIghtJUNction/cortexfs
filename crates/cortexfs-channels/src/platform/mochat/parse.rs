use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::super::{participant, scalar};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget,
    OutboundMessage, OutboundRequest,
};

pub(super) fn one(
    value: &Value,
    channel: ChannelId,
) -> Result<Option<InboundMessage>, ChannelError> {
    let id = value
        .get("messageId")
        .or_else(|| value.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let sender = value
        .get("fromUserId")
        .or_else(|| value.get("sender"))
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let body = value
        .get("content")
        .and_then(|content| {
            content
                .get("text")
                .and_then(Value::as_str)
                .or_else(|| content.as_str())
        })
        .map(str::trim)
        .filter(|text| !text.is_empty());
    let Some(body) = body else {
        return Ok(None);
    };
    let conversation = value
        .get("chatId")
        .or_else(|| value.get("conversationId"))
        .and_then(Value::as_str)
        .unwrap_or(sender);
    let mut metadata = BTreeMap::new();
    if !id.is_empty() {
        metadata.insert("mochat.message_id".to_owned(), id.to_owned());
    }
    Ok(Some(InboundMessage {
        id: if id.is_empty() {
            format!("mochat-{sender}-{body}")
        } else {
            format!("mochat-{id}")
        },
        target: MessageTarget {
            channel,
            conversation: ConversationId::new(conversation.to_owned())?,
            thread: None,
            reply_to: (!id.is_empty()).then(|| id.to_owned()),
        },
        sender: participant(None, sender.to_owned()),
        body: MessageBody::text(body)?,
        timestamp_ms: value.get("timestamp").and_then(|value| {
            scalar(Some(value), "timestamp")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .map(|seconds| seconds.saturating_mul(1_000))
        }),
        metadata,
    }))
}

pub(super) fn outbound(message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
    message.body.validate()?;
    if !message.body.attachments.is_empty() {
        return Err(ChannelError::Unsupported(
            "mochat media attachments".to_owned(),
        ));
    }
    let target = message.target.conversation.as_str();
    Ok(OutboundRequest {
        method: "POST".to_owned(),
        path: "api/message/send".to_owned(),
        content_type: "application/json".to_owned(),
        body: json!({
            "toUserId": target,
            "msgType": "text",
            "content": {"text": message.body.text},
        })
        .to_string(),
        headers: BTreeMap::new(),
    })
}
