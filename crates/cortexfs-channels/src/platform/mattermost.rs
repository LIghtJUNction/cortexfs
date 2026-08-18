use serde_json::{Value, json};

use super::{ChannelCodec, OutboundRequest, object, participant, scalar};
use crate::{
    Attachment, ChannelError, ChannelId, ConversationId, InboundMessage, MessageBody,
    MessageTarget, OutboundMessage,
};

mod effect;
mod event;

/// Mattermost WebSocket `posted` and REST post codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct MattermostCodec;

impl ChannelCodec for MattermostCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("mattermost")
    }

    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError> {
        let root = object(payload)?;
        if root.get("event").and_then(Value::as_str) == Some("posted") {
            let post = root
                .pointer("/data/post")
                .and_then(Value::as_str)
                .ok_or_else(|| ChannelError::Protocol("mattermost post is missing".to_owned()))?;
            return self.decode(post);
        }
        let post = root.get("post").unwrap_or(&root);
        let id = scalar(post.get("id"), "post.id")?;
        let conversation = ConversationId::new(scalar(post.get("channel_id"), "channel_id")?)?;
        let sender_id = scalar(post.get("user_id"), "user_id")?;
        Ok(Some(InboundMessage {
            id,
            target: MessageTarget {
                channel: self.channel(),
                conversation,
                thread: post
                    .get("root_id")
                    .filter(|value| value.as_str().is_some_and(|id| !id.is_empty()))
                    .map(|value| scalar(Some(value), "root_id"))
                    .transpose()?,
                reply_to: None,
            },
            sender: participant(None, sender_id),
            body: MessageBody::with_attachments(
                post.get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default(),
                attachments(post),
            )?,
            timestamp_ms: post.get("create_at").and_then(Value::as_u64),
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
        let mut body = json!({
            "channel_id": message.target.conversation.as_str(),
            "message": message.body.text,
        });
        if let Some(root_id) = message
            .target
            .thread
            .as_deref()
            .or(message.target.reply_to.as_deref())
            && let Some(fields) = body.as_object_mut()
        {
            fields.insert("root_id".to_owned(), json!(root_id));
        }
        if !message.body.attachments.is_empty()
            && let Some(fields) = body.as_object_mut()
        {
            fields.insert(
                "props".to_owned(),
                json!({"attachments": message.body.attachments.iter().map(attachment).collect::<Vec<_>>() }),
            );
        }
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: "api/v4/posts".to_owned(),
            content_type: "application/json".to_owned(),
            body: body.to_string(),
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

pub(super) fn attachments(post: &Value) -> Vec<Attachment> {
    post.pointer("/props/attachments")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            let image = item.get("image_url").and_then(Value::as_str);
            let url = image.or_else(|| item.get("title_link").and_then(Value::as_str))?;
            Some(Attachment {
                url: url.to_owned(),
                name: item.get("title").and_then(Value::as_str).map(str::to_owned),
                mime: image.map(|_| "image/*".to_owned()),
            })
        })
        .collect()
}

fn attachment(item: &Attachment) -> Value {
    let mut value = json!({
        "title": item.name.as_deref().unwrap_or("attachment")
    });
    if let Some(fields) = value.as_object_mut() {
        let key = if item
            .mime
            .as_deref()
            .is_some_and(|mime| mime.starts_with("image/"))
        {
            "image_url"
        } else {
            "title_link"
        };
        fields.insert(key.to_owned(), json!(item.url));
    }
    value
}
