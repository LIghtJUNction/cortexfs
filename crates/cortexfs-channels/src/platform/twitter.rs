use serde_json::{Value, json};

use super::{ChannelCodec, OutboundRequest, object};
use crate::{ChannelError, ChannelId, OutboundMessage};

mod parse;

/// X/Twitter API v2 mentions and reply codec.
#[derive(Clone, Copy, Debug, Default)]
pub struct TwitterCodec;

impl ChannelCodec for TwitterCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("twitter")
    }

    fn decode(&self, payload: &str) -> Result<Option<crate::InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let value = root
            .get("data")
            .and_then(Value::as_array)
            .and_then(|items| items.first())
            .unwrap_or(&root);
        let users = parse::users(&root);
        parse::one(value, self.channel(), &users)
    }

    fn decode_many(&self, payload: &str) -> Result<Vec<crate::InboundMessage>, ChannelError> {
        let root = object(payload)?;
        let users = parse::users(&root);
        root.get("data").and_then(Value::as_array).map_or_else(
            || self.decode(payload).map(|item| item.into_iter().collect()),
            |items| {
                items
                    .iter()
                    .map(|item| parse::one(item, self.channel(), &users))
                    .collect::<Result<Vec<_>, _>>()
                    .map(|items| items.into_iter().flatten().collect())
            },
        )
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        parse::outbound(message)
    }
}

pub(super) fn json_request(path: String, body: &Value) -> OutboundRequest {
    OutboundRequest {
        method: "POST".to_owned(),
        path,
        content_type: "application/json".to_owned(),
        body: body.to_string(),
        headers: std::collections::BTreeMap::new(),
    }
}

pub(super) fn valid_id(value: &str) -> Result<(), ChannelError> {
    if value.is_empty() || value.contains('/') || value.contains('\0') {
        Err(ChannelError::InvalidMessage(
            "twitter id is invalid".to_owned(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn tweet(text: &str, reply_to: Option<&str>) -> Value {
    reply_to.map_or_else(
        || json!({"text": text}),
        |reply_to| {
            json!({
                "text": text,
                "reply": {"in_reply_to_tweet_id": reply_to},
            })
        },
    )
}
