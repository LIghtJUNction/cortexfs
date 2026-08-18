use serde_json::{Value, json};

use super::{ChannelCodec, OutboundRequest, object};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget,
    OutboundMessage, Participant,
};

/// signal-cli JSON envelope codec; the local daemon/process is host-owned.
#[derive(Clone, Copy, Debug, Default)]
pub struct SignalCodec;

impl ChannelCodec for SignalCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("signal")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let envelope = root.get("envelope").unwrap_or(&root);
        let data = envelope
            .get("dataMessage")
            .or_else(|| envelope.get("data_message"));
        let Some(data) = data else { return Ok(None) };
        let text = data
            .get("message")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let Some(text) = text else { return Ok(None) };
        let sender = envelope
            .get("source")
            .or_else(|| envelope.get("sourceNumber"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_owned();
        let group = data
            .get("groupInfo")
            .and_then(|value| value.get("groupId"))
            .and_then(Value::as_str);
        let conversation =
            ConversationId::new(group.map_or_else(|| sender.clone(), str::to_owned))?;
        let mut metadata = std::collections::BTreeMap::new();
        metadata.insert("signal.source".to_owned(), sender.clone());
        if let Some(group) = group {
            metadata.insert("signal.group".to_owned(), group.to_owned());
        }
        Ok(Some(InboundMessage {
            id: envelope
                .get("timestamp")
                .and_then(Value::as_u64)
                .map_or_else(|| sender.clone(), |value| value.to_string()),
            target: MessageTarget {
                channel: self.channel(),
                conversation,
                thread: None,
                reply_to: None,
            },
            sender: Participant {
                id: sender,
                display_name: envelope
                    .get("sourceName")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                handle: None,
            },
            body: MessageBody::text(text)?,
            timestamp_ms: envelope.get("timestamp").and_then(Value::as_u64),
            metadata,
        }))
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        if !message.body.attachments.is_empty() {
            return Err(ChannelError::Unsupported("signal attachments".to_owned()));
        }
        let params = json!({ "recipient": [message.target.conversation.as_str()], "message": message.body.text });
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: "send".to_owned(),
            content_type: "application/json".to_owned(),
            body: json!({"jsonrpc":"2.0","id":"cortexfs","method":"send","params":params})
                .to_string(),
            headers: std::collections::BTreeMap::new(),
        })
    }
}
