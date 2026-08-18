#![expect(
    clippy::field_scoped_visibility_modifiers,
    clippy::redundant_pub_crate,
    reason = "message conversion is private driver plumbing"
)]

use std::collections::BTreeMap;

use cortexfs_channels::{
    ChannelId, ConversationId, InboundMessage, MessageBody, MessageTarget, Participant,
};
use serde_json::Value;

use crate::{config::Config, error::Result};

pub(crate) struct Incoming {
    pub(crate) message: InboundMessage,
    pub(crate) context_token: String,
}

pub(crate) fn decode(value: &Value, config: &Config) -> Result<Option<Incoming>> {
    let user = match value.get("from_user_id").and_then(Value::as_str) {
        Some(value) if !value.trim().is_empty() => value.trim().to_owned(),
        _ => return Ok(None),
    };
    if !config.accepts(&user) {
        return Ok(None);
    }
    let Some((text, kind)) = text_item(value) else {
        return Ok(None);
    };
    let conversation = ConversationId::new(format!("user:{user}"))?;
    let id = value
        .get("message_id")
        .and_then(Value::as_u64)
        .map_or_else(|| fallback_id(value), |id| id.to_string());
    let context_token = value
        .get("context_token")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let mut metadata = BTreeMap::new();
    metadata.insert("wechat_user_id".to_owned(), user.clone());
    metadata.insert("wechat_message_type".to_owned(), kind.to_owned());
    if !context_token.is_empty() {
        metadata.insert("wechat_context_token".to_owned(), context_token.clone());
    }
    Ok(Some(Incoming {
        context_token,
        message: InboundMessage {
            id,
            target: MessageTarget {
                channel: ChannelId::from_static("wechat"),
                conversation,
                thread: None,
                reply_to: None,
            },
            sender: Participant {
                id: user,
                ..Participant::default()
            },
            body: MessageBody::text(text)?,
            timestamp_ms: value.get("create_time_ms").and_then(Value::as_u64),
            metadata,
        },
    }))
}

fn text_item(value: &Value) -> Option<(String, &'static str)> {
    value.get("item_list")?.as_array()?.iter().find_map(|item| {
        let item_type = item.get("type").and_then(Value::as_i64)?;
        let (field, kind) = match item_type {
            1 => ("text_item", "text"),
            3 => ("voice_item", "voice_transcript"),
            _ => return None,
        };
        item.get(field)?
            .get("text")?
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(|text| (text.to_owned(), kind))
    })
}

fn fallback_id(value: &Value) -> String {
    value
        .get("create_time_ms")
        .and_then(Value::as_u64)
        .map_or_else(
            || "wechat-message".to_owned(),
            |value| format!("wechat-{value}"),
        )
}

#[cfg(test)]
mod tests;
