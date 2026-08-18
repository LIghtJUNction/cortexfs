use serde_json::json;

use super::{ChannelCodec, OutboundRequest, object, participant, scalar, text, timestamp_ms};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageTarget, OutboundMessage,
};

mod effect;
mod event;

/// Telegram Bot API update codec. Polling and HTTP transport remain host-owned.
#[derive(Clone, Copy, Debug, Default)]
pub struct TelegramCodec;

impl ChannelCodec for TelegramCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("telegram")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        if root.get("ok").and_then(serde_json::Value::as_bool) == Some(false) {
            return Ok(None);
        }
        let message = root.get("message").or_else(|| root.get("channel_post"));
        let Some(message) = message else {
            return Ok(None);
        };
        let id = scalar(message.get("message_id"), "message_id")?;
        let chat = message
            .get("chat")
            .ok_or_else(|| ChannelError::Protocol("telegram chat is missing".to_owned()))?;
        let conversation = ConversationId::new(scalar(chat.get("id"), "chat.id")?)?;
        let sender_id = message
            .get("from")
            .map(|from| scalar(from.get("id"), "from.id"))
            .transpose()?
            .unwrap_or_else(|| conversation.to_string());
        Ok(Some(InboundMessage {
            id,
            target: MessageTarget {
                channel: self.channel(),
                conversation,
                thread: message
                    .get("message_thread_id")
                    .map(|value| scalar(Some(value), "message_thread_id"))
                    .transpose()?,
                reply_to: message
                    .get("reply_to_message")
                    .and_then(|reply| reply.get("message_id"))
                    .map(|value| scalar(Some(value), "reply_to_message.message_id"))
                    .transpose()?,
            },
            sender: participant(message.get("from"), sender_id),
            body: text(message.get("text").or_else(|| message.get("caption")))?,
            timestamp_ms: timestamp_ms(message.get("date")),
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
        if message.body.attachments.len() > 1 {
            return Err(ChannelError::Unsupported(
                "telegram multiple media attachments".to_owned(),
            ));
        }
        let mut fields = serde_json::Map::new();
        fields.insert(
            "chat_id".to_owned(),
            json!(message.target.conversation.as_str()),
        );
        let path = if let Some(attachment) = message.body.attachments.first() {
            let (path, field) = attachment_path(attachment.mime.as_deref());
            fields.insert(field.to_owned(), json!(attachment.url));
            if !message.body.text.is_empty() {
                fields.insert("caption".to_owned(), json!(message.body.text));
            }
            path
        } else {
            fields.insert("text".to_owned(), json!(message.body.text));
            "sendMessage"
        };
        if let Some(reply) = message.target.reply_to.as_deref() {
            fields.insert("reply_to_message_id".to_owned(), json!(reply));
        }
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: path.to_owned(),
            content_type: "application/json".to_owned(),
            body: serde_json::Value::Object(fields).to_string(),
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

fn attachment_path(mime: Option<&str>) -> (&'static str, &'static str) {
    match mime.unwrap_or_default().split('/').next() {
        Some("image") => ("sendPhoto", "photo"),
        Some("audio") => ("sendAudio", "audio"),
        Some("video") => ("sendVideo", "video"),
        _ => ("sendDocument", "document"),
    }
}
