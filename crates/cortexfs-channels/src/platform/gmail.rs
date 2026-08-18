use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};

use super::{ChannelCodec, OutboundRequest, participant};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget,
    OutboundMessage,
};

mod push;

use push::{body_text, header, push_cursor};

/// Gmail Pub/Sub cursor carried by a push notification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GmailPush {
    pub email_address: String,
    pub history_id: String,
}

/// Gmail API message resource and Pub/Sub send codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct GmailCodec;

impl GmailCodec {
    pub fn push_cursor(payload: &str) -> Result<Option<GmailPush>, ChannelError> {
        push_cursor(payload)
    }
}

impl ChannelCodec for GmailCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("gmail")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = push::message(payload)?;
        if root.get("id").and_then(Value::as_str).is_none() {
            return Ok(None);
        }
        let sender = header(&root, "from").unwrap_or("unknown").to_owned();
        let subject = header(&root, "subject")
            .unwrap_or("(no subject)")
            .to_owned();
        let thread = root
            .get("threadId")
            .and_then(Value::as_str)
            .unwrap_or_else(|| root.get("id").and_then(Value::as_str).unwrap_or("message"));
        let text = body_text(&root).or_else(|| {
            root.get("snippet")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
        let Some(text) = text else {
            return Ok(None);
        };
        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert("email.from".to_owned(), sender.clone());
        metadata.insert("email.subject".to_owned(), subject.clone());
        if let Some(to) = header(&root, "to") {
            metadata.insert("email.to".to_owned(), to.to_owned());
        }
        Ok(Some(InboundMessage {
            id: root
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or(thread)
                .to_owned(),
            target: MessageTarget {
                channel: self.channel(),
                conversation: ConversationId::new(thread)?,
                thread: None,
                reply_to: None,
            },
            sender: participant(None, sender),
            body: MessageBody::text(format!("Subject: {subject}\n\n{text}"))?,
            timestamp_ms: root
                .get("internalDate")
                .and_then(Value::as_str)
                .and_then(|value| value.parse().ok()),
            metadata,
        }))
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        if !message.body.attachments.is_empty() {
            return Err(ChannelError::Unsupported(
                "gmail media attachments".to_owned(),
            ));
        }
        let recipient = message
            .metadata
            .get("email.from")
            .map_or(message.target.conversation.as_str(), String::as_str);
        let subject = message
            .metadata
            .get("email.subject")
            .map_or("CortexFS reply", String::as_str);
        let raw = format!(
            "To: {recipient}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{}",
            message.body.text
        );
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: "users/me/messages/send".to_owned(),
            content_type: "application/json".to_owned(),
            body: json!({"raw": URL_SAFE_NO_PAD.encode(raw)}).to_string(),
            headers: std::collections::BTreeMap::new(),
        })
    }
}
