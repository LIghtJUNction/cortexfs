use serde_json::Value;

use super::{ChannelCodec, OutboundRequest, object, participant, string};
use crate::{
    Attachment, ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody,
    MessageTarget, OutboundMessage,
};

mod effect;
mod encode;
mod event;

/// Slack Events API and `chat.postMessage` codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct SlackCodec;

impl ChannelCodec for SlackCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("slack")
    }

    fn challenge(&self, payload: &str) -> Option<String> {
        let root = serde_json::from_str::<Value>(payload).ok()?;
        (root.get("type").and_then(Value::as_str) == Some("url_verification"))
            .then(|| root.get("challenge").and_then(Value::as_str))
            .flatten()
            .map(str::to_owned)
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let event = root.get("event").unwrap_or(&root);
        if !matches!(
            event.get("type").and_then(Value::as_str),
            Some("message" | "app_mention")
        ) || event.get("subtype").is_some()
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
            body: MessageBody::with_attachments(
                event
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                event
                    .get("files")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|file| {
                        file.get("url_private")
                            .or_else(|| file.get("permalink"))
                            .and_then(Value::as_str)
                            .map(|url| Attachment {
                                url: url.to_owned(),
                                name: file.get("name").and_then(Value::as_str).map(str::to_owned),
                                mime: file
                                    .get("mimetype")
                                    .and_then(Value::as_str)
                                    .map(str::to_owned),
                            })
                    })
                    .collect(),
            )?,
            timestamp_ms: None,
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
        encode::request(message)
    }

    fn encode_effect(
        &self,
        target: &MessageTarget,
        effect: &crate::ChannelEffect,
    ) -> Result<Option<OutboundRequest>, ChannelError> {
        effect::encode(target, effect)
    }
}
