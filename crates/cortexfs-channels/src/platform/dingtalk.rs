use serde_json::{Value, json};

use super::{ChannelCodec, OutboundRequest, object, scalar};
use crate::{
    ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget,
    OutboundMessage, Participant,
};

/// `DingTalk` Stream Mode callback codec. The host owns the gateway socket and
/// keeps each session webhook outside the durable message ABI.
#[derive(Clone, Copy, Debug, Default)]
pub struct DingTalkCodec;

impl DingTalkCodec {
    pub fn session_webhook(payload: &str) -> Option<String> {
        let root = object(payload).ok()?;
        data(&root)?
            .get("sessionWebhook")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    }
}

impl ChannelCodec for DingTalkCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("dingtalk")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        if !matches!(
            root.get("type").and_then(Value::as_str),
            Some("EVENT" | "CALLBACK")
        ) {
            return Ok(None);
        }
        let data = data(&root)
            .ok_or_else(|| ChannelError::Protocol("DingTalk data is missing".to_owned()))?;
        let text = data
            .get("text")
            .and_then(|value| value.get("content"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ChannelError::Protocol("DingTalk text is missing".to_owned()))?;
        let sender = scalar(
            data.get("senderStaffId").or_else(|| data.get("senderId")),
            "senderStaffId",
        )?;
        let conversation = if private_chat(&data) {
            sender.clone()
        } else {
            scalar(data.get("conversationId"), "conversationId")?
        };
        let id = root
            .get("headers")
            .and_then(|value| value.get("messageId"))
            .or_else(|| data.get("msgId"))
            .map(|value| scalar(Some(value), "messageId"))
            .transpose()?
            .unwrap_or_else(|| format!("dingtalk-{sender}"));
        Ok(Some(InboundMessage {
            id,
            target: MessageTarget {
                channel: self.channel(),
                conversation: ConversationId::new(conversation)?,
                thread: None,
                reply_to: None,
            },
            sender: Participant {
                id: sender,
                display_name: data
                    .get("senderNick")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                handle: None,
            },
            body: MessageBody::text(text)?,
            timestamp_ms: None,
            metadata: std::collections::BTreeMap::new(),
        }))
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        if !message.body.attachments.is_empty() {
            return Err(ChannelError::Unsupported(
                "dingtalk media attachments".to_owned(),
            ));
        }
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: "sessionWebhook".to_owned(),
            content_type: "application/json".to_owned(),
            body: json!({"msgtype":"markdown","markdown":{"title":"CortexFS","text":message.body.text}}).to_string(),
            headers: std::collections::BTreeMap::new(),
        })
    }
}

fn data(root: &Value) -> Option<Value> {
    let value = root.get("data")?;
    if let Some(value) = value.as_str() {
        return serde_json::from_str(value).ok();
    }
    value.is_object().then(|| value.clone())
}

fn private_chat(data: &Value) -> bool {
    data.get("conversationType")
        .and_then(|value| {
            value
                .as_str()
                .map(|value| value == "1")
                .or_else(|| value.as_i64().map(|value| value == 1))
        })
        .unwrap_or(true)
}
