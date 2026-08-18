use std::collections::BTreeMap;

use serde_json::{Value, json};

use super::{ChannelCodec, OutboundRequest, object, participant, scalar, timestamp_ms};
use crate::{
    Attachment, ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody,
    MessageTarget, OutboundMessage,
};

/// Microsoft Bot Framework activity codec used by Teams bots.
#[derive(Clone, Copy, Debug, Default)]
pub struct TeamsCodec;

impl ChannelCodec for TeamsCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("teams")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        if root.get("type").and_then(Value::as_str) != Some("message") {
            return Ok(None);
        }
        let text = root.get("text").and_then(Value::as_str).unwrap_or_default();
        let sender = root
            .get("from")
            .ok_or_else(|| ChannelError::Protocol("teams sender is missing".to_owned()))?;
        let sender_id = scalar(sender.get("id"), "from.id")?;
        let conversation = ConversationId::new(scalar(
            root.get("conversation").and_then(|value| value.get("id")),
            "conversation.id",
        )?)?;
        let mut metadata = BTreeMap::new();
        for (key, field) in [
            ("teams.service_url", "serviceUrl"),
            ("teams.channel_id", "channelId"),
        ] {
            if let Some(value) = root.get(field).and_then(Value::as_str) {
                metadata.insert(key.to_owned(), value.to_owned());
            }
        }
        Ok(Some(InboundMessage {
            id: scalar(root.get("id"), "id")?,
            target: MessageTarget {
                channel: self.channel(),
                conversation,
                thread: None,
                reply_to: root
                    .get("replyToId")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            },
            sender: participant(Some(sender), sender_id),
            body: MessageBody::with_attachments(text, attachments(&root))?,
            timestamp_ms: timestamp_ms(root.get("timestamp")),
            metadata,
        }))
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        let mut body = json!({"type":"message","text":message.body.text});
        if let Some(reply) = message.target.reply_to.as_deref()
            && let Some(fields) = body.as_object_mut()
        {
            fields.insert("replyToId".to_owned(), json!(reply));
        }
        if !message.body.attachments.is_empty()
            && let Some(fields) = body.as_object_mut()
        {
            fields.insert(
                "attachments".to_owned(),
                json!(
                    message
                        .body
                        .attachments
                        .iter()
                        .map(attachment)
                        .collect::<Vec<_>>()
                ),
            );
        }
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: format!(
                "v3/conversations/{}/activities",
                message.target.conversation
            ),
            content_type: "application/json".to_owned(),
            body: body.to_string(),
            headers: BTreeMap::new(),
        })
    }
}

fn attachments(root: &Value) -> Vec<Attachment> {
    root.get("attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            Some(Attachment {
                url: item.get("contentUrl").and_then(Value::as_str)?.to_owned(),
                name: item.get("name").and_then(Value::as_str).map(str::to_owned),
                mime: item
                    .get("contentType")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            })
        })
        .collect()
}

fn attachment(item: &Attachment) -> Value {
    json!({
        "contentType": item.mime.as_deref().unwrap_or("application/octet-stream"),
        "contentUrl": item.url,
        "name": item.name.as_deref().unwrap_or("attachment")
    })
}
