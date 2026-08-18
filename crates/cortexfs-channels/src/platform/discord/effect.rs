use serde_json::json;

use crate::{ChannelEffect, ChannelError, MessageTarget, OutboundRequest};

#[expect(
    clippy::pattern_type_mismatch,
    reason = "match ergonomics keep borrowed effect fields readable"
)]
pub(super) fn encode(
    target: &MessageTarget,
    effect: &ChannelEffect,
) -> Result<Option<OutboundRequest>, ChannelError> {
    effect.validate()?;
    let channel = target.conversation.as_str();
    let request = match effect {
        ChannelEffect::Typing { active: true } => Some(request(
            "POST",
            &format!("channels/{}/typing", segment(channel)),
            &json!({}),
        )),
        ChannelEffect::Reaction {
            message_id,
            emoji,
            remove,
        } => {
            let operation = if *remove { "DELETE" } else { "PUT" };
            Some(request(
                operation,
                &format!(
                    "channels/{}/messages/{}/reactions/{}/@me",
                    segment(channel),
                    segment(message_id),
                    segment(emoji)
                ),
                &json!({}),
            ))
        }
        ChannelEffect::Edit { message_id, body } => {
            body.validate()?;
            Some(request(
                "PATCH",
                &format!(
                    "channels/{}/messages/{}",
                    segment(channel),
                    segment(message_id)
                ),
                &json!({"content":body.text,"allowed_mentions":{"parse":[]}}),
            ))
        }
        ChannelEffect::Delete { message_id } | ChannelEffect::Redact { message_id, .. } => {
            Some(request(
                "DELETE",
                &format!(
                    "channels/{}/messages/{}",
                    segment(channel),
                    segment(message_id)
                ),
                &json!({}),
            ))
        }
        ChannelEffect::Pin { message_id } => Some(request(
            "PUT",
            &format!("channels/{}/pins/{}", segment(channel), segment(message_id)),
            &json!({}),
        )),
        ChannelEffect::Unpin { message_id } => Some(request(
            "DELETE",
            &format!("channels/{}/pins/{}", segment(channel), segment(message_id)),
            &json!({}),
        )),
        ChannelEffect::Typing { active: false }
        | ChannelEffect::Preview { .. }
        | ChannelEffect::MarkRead { .. } => None,
    };
    Ok(request)
}

fn request(method: &str, path: &str, body: &serde_json::Value) -> OutboundRequest {
    OutboundRequest {
        method: method.to_owned(),
        path: path.to_owned(),
        content_type: "application/json".to_owned(),
        body: body.to_string(),
        headers: std::collections::BTreeMap::new(),
    }
}

fn segment(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}
