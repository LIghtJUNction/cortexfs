use serde_json::{Value, json};

use super::{ChannelCodec, OutboundRequest, object, participant, scalar, string};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget,
    OutboundMessage,
};

/// Feishu/Lark event and `im.v1.message.create` codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct FeishuCodec;

impl ChannelCodec for FeishuCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("feishu")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let event = root.get("event").unwrap_or(&root);
        let message = event
            .get("message")
            .ok_or_else(|| ChannelError::Protocol("feishu message is missing".to_owned()))?;
        let body = message
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ChannelError::Protocol("feishu message.content is missing".to_owned())
            })?;
        let body: Value = serde_json::from_str(body)
            .map_err(|error| ChannelError::Protocol(format!("invalid feishu content: {error}")))?;
        let sender = event.get("sender").and_then(|value| value.get("sender_id"));
        let sender_id = sender
            .and_then(|value| value.get("open_id").or_else(|| value.get("user_id")))
            .map(|value| scalar(Some(value), "sender.sender_id"))
            .transpose()?
            .unwrap_or_else(|| "unknown".to_owned());
        let conversation = ConversationId::new(string(message.get("chat_id"), "message.chat_id")?)?;
        Ok(Some(InboundMessage {
            id: string(message.get("message_id"), "message.message_id")?,
            target: MessageTarget {
                channel: self.channel(),
                conversation,
                thread: message
                    .get("root_id")
                    .map(|value| string(Some(value), "message.root_id"))
                    .transpose()?,
                reply_to: None,
            },
            sender: participant(None, sender_id),
            body: MessageBody::text(string(body.get("text"), "content.text")?)?,
            timestamp_ms: None,
            metadata: std::collections::BTreeMap::new(),
        }))
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        if !message.body.attachments.is_empty() {
            return Err(ChannelError::Unsupported(
                "feishu media attachments".to_owned(),
            ));
        }
        let body = json!({
            "receive_id": message.target.conversation.as_str(),
            "msg_type": "text",
            "content": json!({ "text": message.body.text }).to_string(),
        });
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: "im/v1/messages".to_owned(),
            content_type: "application/json".to_owned(),
            body: body.to_string(),
            headers: std::collections::BTreeMap::new(),
        })
    }
}
