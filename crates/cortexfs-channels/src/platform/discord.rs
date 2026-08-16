use serde_json::json;

use super::{ChannelCodec, OutboundRequest, object, participant, scalar, text};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageTarget, OutboundMessage,
};

/// Discord message/webhook codec. Gateway authentication and websocket choice remain host-owned.
#[derive(Clone, Copy, Debug, Default)]
pub struct DiscordCodec;

impl ChannelCodec for DiscordCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("discord")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        if root
            .get("author")
            .and_then(|author| author.get("bot"))
            .and_then(serde_json::Value::as_bool)
            == Some(true)
        {
            return Ok(None);
        }
        let id = scalar(root.get("id"), "id")?;
        let conversation = ConversationId::new(scalar(root.get("channel_id"), "channel_id")?)?;
        Ok(Some(InboundMessage {
            id,
            target: MessageTarget {
                channel: self.channel(),
                conversation,
                thread: root
                    .get("thread_id")
                    .map(|value| scalar(Some(value), "thread_id"))
                    .transpose()?,
                reply_to: root
                    .get("message_reference")
                    .and_then(|reference| reference.get("message_id"))
                    .map(|value| scalar(Some(value), "message_reference.message_id"))
                    .transpose()?,
            },
            sender: participant(
                root.get("author"),
                scalar(
                    root.get("author").and_then(|value| value.get("id")),
                    "author.id",
                )?,
            ),
            body: text(root.get("content"))?,
            timestamp_ms: None,
            metadata: std::collections::BTreeMap::new(),
        }))
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        if !message.body.attachments.is_empty() {
            return Err(ChannelError::Unsupported(
                "discord media attachments".to_owned(),
            ));
        }
        let mut fields = serde_json::Map::new();
        fields.insert("content".to_owned(), json!(message.body.text));
        fields.insert("allowed_mentions".to_owned(), json!({ "parse": [] }));
        if let Some(thread) = message.target.thread.as_deref() {
            fields.insert("thread_id".to_owned(), json!(thread));
        }
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: "webhook".to_owned(),
            content_type: "application/json".to_owned(),
            body: serde_json::Value::Object(fields).to_string(),
            headers: std::collections::BTreeMap::new(),
        })
    }
}
