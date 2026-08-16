use serde_json::json;

use super::{ChannelCodec, OutboundRequest, object, participant, string, text};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageTarget, OutboundMessage,
};

/// Slack Events API and `chat.postMessage` codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct SlackCodec;

impl ChannelCodec for SlackCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("slack")
    }

    fn challenge(&self, payload: &str) -> Option<String> {
        let root = serde_json::from_str::<serde_json::Value>(payload).ok()?;
        (root.get("type").and_then(serde_json::Value::as_str) == Some("url_verification"))
            .then(|| root.get("challenge").and_then(serde_json::Value::as_str))
            .flatten()
            .map(str::to_owned)
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let event = root.get("event").unwrap_or(&root);
        if event.get("type").and_then(serde_json::Value::as_str) != Some("message")
            || event.get("subtype").is_some()
            || event.get("bot_id").is_some()
        {
            return Ok(None);
        }
        let id = string(event.get("ts"), "event.ts")?;
        let conversation = ConversationId::new(string(event.get("channel"), "event.channel")?)?;
        let sender_id = string(event.get("user"), "event.user")?;
        Ok(Some(InboundMessage {
            id,
            target: MessageTarget {
                channel: self.channel(),
                conversation,
                thread: event
                    .get("thread_ts")
                    .map(|value| string(Some(value), "event.thread_ts"))
                    .transpose()?,
                reply_to: None,
            },
            sender: participant(None, sender_id),
            body: text(event.get("text"))?,
            timestamp_ms: None,
            metadata: std::collections::BTreeMap::new(),
        }))
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        if !message.body.attachments.is_empty() {
            return Err(ChannelError::Unsupported(
                "slack media attachments".to_owned(),
            ));
        }
        let mut fields = serde_json::Map::new();
        fields.insert(
            "channel".to_owned(),
            json!(message.target.conversation.as_str()),
        );
        fields.insert("text".to_owned(), json!(message.body.text));
        if let Some(thread) = message.target.thread.as_deref() {
            fields.insert("thread_ts".to_owned(), json!(thread));
        }
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: "chat.postMessage".to_owned(),
            content_type: "application/json".to_owned(),
            body: serde_json::Value::Object(fields).to_string(),
            headers: std::collections::BTreeMap::new(),
        })
    }
}
