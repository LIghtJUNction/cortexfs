use serde_json::Value;

use super::{ChannelCodec, OutboundRequest, object, participant, scalar, string};
use crate::{
    Attachment, ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody,
    MessageTarget, OutboundMessage,
};

mod effect;
mod encode;
mod event;

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
            .and_then(Value::as_bool)
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
            body: MessageBody::with_attachments(
                root.get("content")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                root.get("attachments")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|attachment| {
                        Ok(Attachment {
                            url: string(attachment.get("url"), "attachments.url")?,
                            name: attachment
                                .get("filename")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                            mime: attachment
                                .get("content_type")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                        })
                    })
                    .collect::<Result<Vec<_>, ChannelError>>()?,
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
