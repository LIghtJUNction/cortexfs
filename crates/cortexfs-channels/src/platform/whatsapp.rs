use serde_json::{Value, json};

use super::{ChannelCodec, OutboundRequest, object, participant, scalar, string};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget,
    OutboundMessage,
};

/// `WhatsApp` Business Cloud webhook and Graph API message codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct WhatsAppCodec;

impl ChannelCodec for WhatsAppCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("whatsapp")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let message = root
            .get("entry")
            .and_then(Value::as_array)
            .and_then(|entries| entries.first())
            .and_then(|entry| entry.get("changes"))
            .and_then(Value::as_array)
            .and_then(|changes| changes.first())
            .and_then(|change| change.get("value"))
            .and_then(|value| value.get("messages"))
            .and_then(Value::as_array)
            .and_then(|messages| messages.first());
        let Some(message) = message else {
            return Ok(None);
        };
        if message.get("type").and_then(Value::as_str) != Some("text") {
            return Ok(None);
        }
        let sender = scalar(message.get("from"), "messages.from")?;
        let sender = format_phone(&sender);
        Ok(Some(InboundMessage {
            id: string(message.get("id"), "messages.id")?,
            target: MessageTarget {
                channel: self.channel(),
                conversation: ConversationId::new(sender.clone())?,
                thread: None,
                reply_to: None,
            },
            sender: participant(None, sender),
            body: MessageBody::text(string(
                message.get("text").and_then(|value| value.get("body")),
                "messages.text.body",
            )?)?,
            timestamp_ms: message
                .get("timestamp")
                .and_then(Value::as_str)
                .and_then(|value| value.parse::<u64>().ok())
                .map(|seconds| seconds.saturating_mul(1_000)),
            metadata: std::collections::BTreeMap::new(),
        }))
    }

    fn decode_many(&self, payload: &str) -> Result<Vec<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let mut result = Vec::new();
        for entry in root
            .get("entry")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            for change in entry
                .get("changes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                for message in change
                    .get("value")
                    .and_then(|value| value.get("messages"))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    let payload = json!({"entry":[{"changes":[{"value":{"messages":[message]}}]}]});
                    if let Some(message) = self.decode(&payload.to_string())? {
                        result.push(message);
                    }
                }
            }
        }
        Ok(result)
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        if message.body.attachments.len() > 1 {
            return Err(ChannelError::Unsupported(
                "whatsapp multiple media attachments".to_owned(),
            ));
        }
        let mut body = json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": message.target.conversation.as_str().trim_start_matches('+'),
            "type": "text",
            "text": {"preview_url": false, "body": message.body.text}
        });
        if let Some(attachment) = message.body.attachments.first()
            && let Some(fields) = body.as_object_mut()
        {
            let kind = attachment
                .mime
                .as_deref()
                .and_then(|mime| mime.split('/').next())
                .filter(|kind| matches!(*kind, "image" | "audio" | "video"))
                .unwrap_or("document");
            fields.insert("type".to_owned(), json!(kind));
            let mut media = json!({"link": attachment.url});
            if !message.body.text.is_empty()
                && let Some(media_fields) = media.as_object_mut()
            {
                media_fields.insert("caption".to_owned(), json!(message.body.text));
            }
            fields.insert(kind.to_owned(), media);
            fields.remove("text");
        }
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: "messages".to_owned(),
            content_type: "application/json".to_owned(),
            body: body.to_string(),
            headers: std::collections::BTreeMap::new(),
        })
    }
}

fn format_phone(value: &str) -> String {
    value
        .strip_prefix('+')
        .map_or_else(|| format!("+{value}"), str::to_owned)
}
