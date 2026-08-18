use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{ChannelCodec, OutboundRequest, object, participant, scalar, text};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageTarget, OutboundMessage,
};

/// QQ Bot API gateway events and message-send codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct QqCodec;

impl ChannelCodec for QqCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("qq")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let event = root.get("t").and_then(Value::as_str);
        if !matches!(
            event,
            Some("AT_MESSAGE_CREATE" | "DIRECT_MESSAGE_CREATE" | "GROUP_AT_MESSAGE_CREATE")
        ) {
            return Ok(None);
        }
        let author = root.get("author");
        if author
            .and_then(|value| value.get("bot"))
            .and_then(Value::as_bool)
            == Some(true)
        {
            return Ok(None);
        }
        let sender_id = author
            .and_then(|value| value.get("id").or_else(|| value.get("user_openid")))
            .map(|value| scalar(Some(value), "author.id"))
            .transpose()?
            .or_else(|| {
                root.get("openid")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
            .ok_or_else(|| ChannelError::Protocol("qq sender is missing".to_owned()))?;
        let (conversation, kind) = conversation(&root, event)?;
        let mut metadata = BTreeMap::new();
        metadata.insert("qq.target_kind".to_owned(), kind.to_owned());
        Ok(Some(InboundMessage {
            id: scalar(root.get("id"), "id")?,
            target: MessageTarget {
                channel: self.channel(),
                conversation,
                thread: root
                    .get("message_reference")
                    .and_then(|value| value.get("message_id"))
                    .map(|value| scalar(Some(value), "message_reference.message_id"))
                    .transpose()?,
                reply_to: None,
            },
            sender: participant(author, sender_id),
            body: text(root.get("content"))?,
            timestamp_ms: None,
            metadata,
        }))
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        if !message.body.attachments.is_empty() {
            return Err(ChannelError::Unsupported("qq media attachments".to_owned()));
        }
        let kind = message
            .metadata
            .get("qq.target_kind")
            .map_or("guild", String::as_str);
        let path = match kind {
            "c2c" => format!("v2/users/{}/messages", message.target.conversation),
            "group" => format!("v2/groups/{}/messages", message.target.conversation),
            "guild" => format!("channels/{}/messages", message.target.conversation),
            other => return Err(ChannelError::InvalidValue(other.to_owned())),
        };
        let mut body = json!({"content": message.body.text, "msg_type": 0, "msg_seq": 1});
        if let Some(reply) = message
            .target
            .reply_to
            .as_deref()
            .or(message.target.thread.as_deref())
            && let Some(fields) = body.as_object_mut()
        {
            fields.insert("msg_id".to_owned(), json!(reply));
        }
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path,
            content_type: "application/json".to_owned(),
            body: body.to_string(),
            headers: BTreeMap::new(),
        })
    }
}

fn conversation<'a>(
    root: &'a Value,
    event: Option<&str>,
) -> Result<(ConversationId, &'a str), ChannelError> {
    let (field, kind) = match event {
        Some("GROUP_AT_MESSAGE_CREATE") => ("group_openid", "group"),
        Some("DIRECT_MESSAGE_CREATE") => ("openid", "c2c"),
        _ => ("channel_id", "guild"),
    };
    Ok((ConversationId::new(scalar(root.get(field), field)?)?, kind))
}
