use serde_json::{Value, json};

use super::{ChannelCodec, OutboundRequest, object, participant, scalar};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget,
    OutboundMessage,
};

mod effect;
mod event;

/// Matrix Client-Server `m.room.message` event codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct MatrixCodec;

impl ChannelCodec for MatrixCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("matrix")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        if root.get("type").and_then(Value::as_str) != Some("m.room.message") {
            return Ok(None);
        }
        let content = root
            .get("content")
            .ok_or_else(|| ChannelError::Protocol("matrix content is missing".to_owned()))?;
        let body = scalar(content.get("body"), "content.body")?;
        let sender = scalar(root.get("sender"), "sender")?;
        let conversation = ConversationId::new(scalar(root.get("room_id"), "room_id")?)?;
        let relation = content.get("m.relates_to");
        let thread = relation
            .filter(|value| value.get("rel_type").and_then(Value::as_str) == Some("m.thread"))
            .and_then(|value| value.get("event_id"))
            .map(|value| scalar(Some(value), "m.relates_to.event_id"))
            .transpose()?;
        let reply_to = relation
            .and_then(|value| value.get("m.in_reply_to"))
            .and_then(|value| value.get("event_id"))
            .map(|value| scalar(Some(value), "m.in_reply_to.event_id"))
            .transpose()?;
        Ok(Some(InboundMessage {
            id: scalar(root.get("event_id"), "event_id")?,
            target: MessageTarget {
                channel: self.channel(),
                conversation,
                thread,
                reply_to,
            },
            sender: participant(None, sender),
            body: MessageBody::text(body)?,
            timestamp_ms: root.get("origin_server_ts").and_then(Value::as_u64),
            metadata: std::collections::BTreeMap::new(),
        }))
    }

    fn decode_event(
        &self,
        payload: &str,
    ) -> Result<Option<crate::ChannelIncomingEvent>, ChannelError> {
        event::decode(payload, self.channel())
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        if !message.body.attachments.is_empty() {
            return Err(ChannelError::Unsupported(
                "matrix media attachments".to_owned(),
            ));
        }
        let mut content = json!({"msgtype":"m.text","body":message.body.text});
        let relation = message.target.thread.as_deref().map_or_else(
            || {
                message
                    .target
                    .reply_to
                    .as_deref()
                    .map(|reply| json!({"m.in_reply_to":{"event_id":reply}}))
            },
            |thread| {
                let mut relation = json!({"rel_type":"m.thread","event_id":thread});
                if let Some(reply) = message.target.reply_to.as_deref()
                    && let Some(relation) = relation.as_object_mut()
                {
                    relation.insert("m.in_reply_to".to_owned(), json!({"event_id":reply}));
                }
                Some(relation)
            },
        );
        if let Some(relation) = relation
            && let Some(content) = content.as_object_mut()
        {
            content.insert("m.relates_to".to_owned(), relation);
        }
        Ok(OutboundRequest {
            method: "PUT".to_owned(),
            path: format!("rooms/{}/send/m.room.message", message.target.conversation),
            content_type: "application/json".to_owned(),
            body: content.to_string(),
            headers: std::collections::BTreeMap::new(),
        })
    }

    fn encode_effect(
        &self,
        target: &MessageTarget,
        effect: &crate::ChannelEffect,
    ) -> Result<Option<OutboundRequest>, ChannelError> {
        effect::encode(target, effect)
    }
}
