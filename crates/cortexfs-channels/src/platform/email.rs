use serde_json::json;

use super::{ChannelCodec, OutboundRequest, participant};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget,
    OutboundMessage,
};

mod parse;
mod wire;

/// IMAP/SMTP message codec. Hosts may pass JSON resources or RFC 5322 text.
#[derive(Clone, Copy, Debug, Default)]
pub struct EmailCodec;

impl ChannelCodec for EmailCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("email")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let mail = parse::parse(payload)?;
        let Some(mail) = mail else { return Ok(None) };
        let sender = mail.from.clone().unwrap_or_else(|| "unknown".to_owned());
        let conversation =
            ConversationId::new(mail.thread.clone().unwrap_or_else(|| sender.clone()))?;
        let mut metadata = mail.metadata;
        metadata.insert("email.from".to_owned(), sender.clone());
        if let Some(value) = mail.subject.clone() {
            metadata.insert("email.subject".to_owned(), value);
        }
        Ok(Some(InboundMessage {
            id: mail.id.unwrap_or_else(|| conversation.to_string()),
            target: MessageTarget {
                channel: self.channel(),
                conversation,
                thread: mail.thread,
                reply_to: mail.reply_to,
            },
            sender: participant(None, sender),
            body: MessageBody::text(mail.body)?,
            timestamp_ms: mail.timestamp_ms,
            metadata,
        }))
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        if !message.body.attachments.is_empty() {
            return Err(ChannelError::Unsupported("email attachments".to_owned()));
        }
        let to = message
            .metadata
            .get("email.from")
            .map_or(message.target.conversation.as_str(), String::as_str);
        let subject = message
            .metadata
            .get("email.subject")
            .map_or("CortexFS reply", String::as_str);
        let mut headers = serde_json::Map::new();
        headers.insert("to".to_owned(), json!(to));
        headers.insert("subject".to_owned(), json!(subject));
        if let Some(reply) = message.target.reply_to.as_deref() {
            headers.insert("in_reply_to".to_owned(), json!(reply));
        }
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: "smtp".to_owned(),
            content_type: "message/rfc822".to_owned(),
            body: wire::rfc822(headers, &message.body.text),
            headers: std::collections::BTreeMap::new(),
        })
    }
}
