use serde_json::{Value, json};

use super::{ChannelCodec, OutboundRequest, object};
use crate::{ChannelError, ChannelId, OutboundMessage};

mod parse;

/// Linq Partner API webhook and chat-message codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct LinqCodec;

impl ChannelCodec for LinqCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("linq")
    }

    fn decode(&self, payload: &str) -> Result<Option<crate::InboundMessage>, ChannelError> {
        parse::decode(&object(payload)?, self.channel())
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        let conversation = message.target.conversation.as_str();
        if conversation.is_empty()
            || conversation
                .bytes()
                .any(|byte| matches!(byte, b'/' | b'\\' | b'?' | b'#' | 0))
        {
            return Err(ChannelError::InvalidMessage(
                "linq conversation id is not a safe path segment".to_owned(),
            ));
        }
        let mut parts = Vec::with_capacity(1 + message.body.attachments.len());
        if !message.body.text.is_empty() {
            parts.push(json!({"type":"text","value":message.body.text}));
        }
        for attachment in &message.body.attachments {
            let mut part = json!({"type":"media","url":attachment.url});
            if let Some(fields) = part.as_object_mut() {
                if let Some(name) = attachment.name.as_deref() {
                    fields.insert("name".to_owned(), json!(name));
                }
                if let Some(mime) = attachment.mime.as_deref() {
                    fields.insert("mime_type".to_owned(), json!(mime));
                }
            }
            parts.push(part);
        }
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: format!("chats/{conversation}/messages"),
            content_type: "application/json".to_owned(),
            body: serde_json::json!({
                "message": {"parts": parts}
            })
            .to_string(),
            headers: std::collections::BTreeMap::new(),
        })
    }

    fn challenge(&self, payload: &str) -> Option<String> {
        let root: Value = serde_json::from_str(payload).ok()?;
        root.get("challenge")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }
}
