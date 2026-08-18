use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{ChannelCodec, OutboundRequest, object, participant, scalar, string};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget,
    OutboundMessage,
};

mod encode;

/// LINE Messaging API webhook and reply/push codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct LineCodec;

impl ChannelCodec for LineCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("line")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let event = root
            .get("events")
            .and_then(Value::as_array)
            .and_then(|events| events.first());
        let Some(event) = event else {
            return Ok(None);
        };
        if event.get("type").and_then(Value::as_str) != Some("message")
            || event
                .get("message")
                .and_then(|message| message.get("type"))
                .and_then(Value::as_str)
                != Some("text")
        {
            return Ok(None);
        }
        let source = event
            .get("source")
            .ok_or_else(|| ChannelError::Protocol("line source is missing".to_owned()))?;
        let sender = string(source.get("userId"), "source.userId")?;
        let conversation = source
            .get("groupId")
            .or_else(|| source.get("roomId"))
            .and_then(Value::as_str)
            .unwrap_or(&sender)
            .to_owned();
        let mut metadata = BTreeMap::new();
        if let Some(token) = event.get("replyToken").and_then(Value::as_str) {
            metadata.insert("line.reply_token".to_owned(), token.to_owned());
        }
        metadata.insert(
            "line.source_type".to_owned(),
            source
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("user")
                .to_owned(),
        );
        Ok(Some(InboundMessage {
            id: scalar(event.get("message").and_then(|m| m.get("id")), "message.id")?,
            target: MessageTarget {
                channel: self.channel(),
                conversation: ConversationId::new(conversation)?,
                thread: None,
                reply_to: None,
            },
            sender: participant(None, sender),
            body: MessageBody::text(string(
                event.get("message").and_then(|m| m.get("text")),
                "message.text",
            )?)?,
            timestamp_ms: event.get("timestamp").and_then(Value::as_u64),
            metadata,
        }))
    }

    fn decode_many(&self, payload: &str) -> Result<Vec<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let Some(events) = root.get("events").and_then(Value::as_array) else {
            return Ok(Vec::new());
        };
        events
            .iter()
            .map(|event| self.decode(&json!({"events": [event]}).to_string()))
            .collect::<Result<Vec<_>, _>>()
            .map(|messages| messages.into_iter().flatten().collect())
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        encode::request(message)
    }
}
