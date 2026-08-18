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
    let room = target.conversation.as_str();
    let request = match effect {
        ChannelEffect::Reaction {
            message_id,
            emoji,
            remove,
        } => {
            if *remove {
                None
            } else {
                Some(send(
                    room,
                    "m.reaction",
                    &json!({"m.relates_to":{"rel_type":"m.annotation","event_id":message_id,"key":emoji}}),
                ))
            }
        }
        ChannelEffect::Edit { message_id, body } => {
            body.validate()?;
            Some(send(
                room,
                "m.room.message",
                &json!({
                    "msgtype":"m.text",
                    "body":body.text,
                    "m.new_content":{"msgtype":"m.text","body":body.text},
                    "m.relates_to":{"rel_type":"m.replace","event_id":message_id}
                }),
            ))
        }
        ChannelEffect::Delete { message_id } | ChannelEffect::Redact { message_id, .. } => {
            Some(OutboundRequest {
                method: "PUT".to_owned(),
                path: format!("rooms/{room}/redact/{message_id}"),
                content_type: "application/json".to_owned(),
                body: "{}".to_owned(),
                headers: std::collections::BTreeMap::new(),
            })
        }
        ChannelEffect::MarkRead { message_id } => Some(OutboundRequest {
            method: "POST".to_owned(),
            path: format!("rooms/{room}/read_markers"),
            content_type: "application/json".to_owned(),
            body: json!({"m.fully_read":message_id,"m.read":message_id}).to_string(),
            headers: std::collections::BTreeMap::new(),
        }),
        ChannelEffect::Typing { .. }
        | ChannelEffect::Preview { .. }
        | ChannelEffect::Pin { .. }
        | ChannelEffect::Unpin { .. } => None,
    };
    Ok(request)
}

fn send(room: &str, kind: &str, content: &serde_json::Value) -> OutboundRequest {
    OutboundRequest {
        method: "PUT".to_owned(),
        path: format!("rooms/{room}/send/{kind}"),
        content_type: "application/json".to_owned(),
        body: content.to_string(),
        headers: std::collections::BTreeMap::new(),
    }
}
