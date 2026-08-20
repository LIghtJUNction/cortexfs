use serde_json::{Value, json};

use super::{ChannelCodec, OutboundRequest, object};
use crate::{ChannelError, ChannelId, OutboundMessage};

mod parse;

/// AT Protocol notification and post codec. Authentication and polling remain
/// host-owned so the public crate stays runtime-neutral.
#[derive(Clone, Copy, Debug, Default)]
pub struct BlueskyCodec;

impl ChannelCodec for BlueskyCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("bluesky")
    }

    fn decode(&self, payload: &str) -> Result<Option<crate::InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let value = root
            .get("notifications")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .unwrap_or(&root);
        parse::one(value, self.channel())
    }

    fn decode_many(&self, payload: &str) -> Result<Vec<crate::InboundMessage>, ChannelError> {
        let root = object(payload)?;
        root.get("notifications")
            .and_then(Value::as_array)
            .map_or_else(
                || self.decode(payload).map(|item| item.into_iter().collect()),
                |items| {
                    items
                        .iter()
                        .map(|item| parse::one(item, self.channel()))
                        .collect::<Result<Vec<_>, _>>()
                        .map(|items| items.into_iter().flatten().collect())
                },
            )
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        if !message.body.attachments.is_empty() {
            return Err(ChannelError::Unsupported(
                "bluesky media attachments".to_owned(),
            ));
        }
        if message.body.text.chars().count() > 300 {
            return Err(ChannelError::Unsupported(
                "bluesky message exceeds 300 characters".to_owned(),
            ));
        }
        let repo = message
            .metadata
            .get("bluesky.repo")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ChannelError::InvalidMessage("bluesky repo is missing".to_owned()))?;
        let reply = parse::reply(message);
        let created_at = message
            .metadata
            .get("bluesky.created_at")
            .filter(|value| !value.is_empty())
            .ok_or_else(|| {
                ChannelError::InvalidMessage("bluesky created_at is missing".to_owned())
            })?;
        let mut record = json!({
            "$type": "app.bsky.feed.post",
            "text": message.body.text,
            "createdAt": created_at,
            "reply": reply,
        });
        if let (Some(uri), Some(cid)) = (
            message.metadata.get("bluesky.quote_uri"),
            message.metadata.get("bluesky.quote_cid"),
        ) && let Some(fields) = record.as_object_mut()
        {
            fields.insert(
                "embed".to_owned(),
                json!({
                    "$type": "app.bsky.embed.record",
                    "record": {"uri": uri, "cid": cid},
                }),
            );
        }
        Ok(OutboundRequest {
            method: "POST".to_owned(),
            path: "com.atproto.repo.createRecord".to_owned(),
            content_type: "application/json".to_owned(),
            body: json!({
                "repo": repo,
                "collection": "app.bsky.feed.post",
                "record": record,
            })
            .to_string(),
            headers: std::collections::BTreeMap::new(),
        })
    }
}
