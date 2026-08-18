use serde_json::json;

use crate::{ChannelEffect, ChannelError, MessageTarget, OutboundRequest};

#[expect(
    clippy::pattern_type_mismatch,
    reason = "matching borrowed effects keeps provider request fields borrowed"
)]
pub(super) fn encode(
    _target: &MessageTarget,
    effect: &ChannelEffect,
) -> Result<Option<OutboundRequest>, ChannelError> {
    effect.validate()?;
    let request = match effect {
        ChannelEffect::Reaction {
            message_id,
            emoji,
            remove: false,
        } => Some(request(
            "POST",
            "api/v4/reactions".to_owned(),
            &json!({"user_id":"me","post_id":message_id,"emoji_name":emoji}),
        )),
        ChannelEffect::Reaction {
            message_id,
            emoji,
            remove: true,
        } => Some(request(
            "DELETE",
            format!(
                "api/v4/users/me/posts/{}/reactions/{}",
                segment(message_id),
                segment(emoji)
            ),
            &json!({}),
        )),
        ChannelEffect::Edit { message_id, body } => {
            body.validate()?;
            Some(request(
                "PUT",
                format!("api/v4/posts/{}", segment(message_id)),
                &json!({"id":message_id,"message":body.text}),
            ))
        }
        ChannelEffect::Delete { message_id } | ChannelEffect::Redact { message_id, .. } => {
            Some(request(
                "DELETE",
                format!("api/v4/posts/{}", segment(message_id)),
                &json!({}),
            ))
        }
        ChannelEffect::Pin { message_id } => Some(request(
            "POST",
            format!("api/v4/posts/{}/pin", segment(message_id)),
            &json!({}),
        )),
        ChannelEffect::Unpin { message_id } => Some(request(
            "POST",
            format!("api/v4/posts/{}/unpin", segment(message_id)),
            &json!({}),
        )),
        ChannelEffect::Typing { .. }
        | ChannelEffect::Preview { .. }
        | ChannelEffect::MarkRead { .. } => None,
    };
    Ok(request)
}

fn request(method: &str, path: String, body: &serde_json::Value) -> OutboundRequest {
    OutboundRequest {
        method: method.to_owned(),
        path,
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
