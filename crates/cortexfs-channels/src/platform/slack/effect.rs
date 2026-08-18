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
        ChannelEffect::Reaction {
            message_id,
            emoji,
            remove,
        } => Some(OutboundRequest {
            method: "POST".to_owned(),
            path: if *remove {
                "reactions.remove".to_owned()
            } else {
                "reactions.add".to_owned()
            },
            content_type: "application/json".to_owned(),
            body: json!({"channel":channel,"timestamp":message_id,"name":emoji}).to_string(),
            headers: std::collections::BTreeMap::new(),
        }),
        ChannelEffect::Edit { message_id, body } => {
            body.validate()?;
            Some(request(
                "chat.update",
                &json!({
                    "channel": channel,
                    "ts": message_id,
                    "text": body.text,
                }),
            ))
        }
        ChannelEffect::Delete { message_id } | ChannelEffect::Redact { message_id, .. } => Some(
            request("chat.delete", &json!({"channel":channel,"ts":message_id})),
        ),
        ChannelEffect::Pin { message_id } => Some(request(
            "pins.add",
            &json!({"channel":channel,"timestamp":message_id}),
        )),
        ChannelEffect::Unpin { message_id } => Some(request(
            "pins.remove",
            &json!({"channel":channel,"timestamp":message_id}),
        )),
        ChannelEffect::Typing { .. }
        | ChannelEffect::Preview { .. }
        | ChannelEffect::MarkRead { .. } => None,
    };
    Ok(request)
}

fn request(path: &str, body: &serde_json::Value) -> OutboundRequest {
    OutboundRequest {
        method: "POST".to_owned(),
        path: path.to_owned(),
        content_type: "application/json".to_owned(),
        body: body.to_string(),
        headers: std::collections::BTreeMap::new(),
    }
}
