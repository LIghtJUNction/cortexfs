use serde_json::{Map, json};

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
    let mut fields = Map::new();
    fields.insert("chat_id".to_owned(), json!(target.conversation.as_str()));
    let request = match effect {
        ChannelEffect::Typing { active: true } => {
            fields.insert("action".to_owned(), json!("typing"));
            Some(request("sendChatAction", fields))
        }
        ChannelEffect::Reaction {
            message_id,
            emoji,
            remove,
        } => {
            fields.insert("message_id".to_owned(), json!(message_id));
            fields.insert(
                "reaction".to_owned(),
                if *remove {
                    json!([])
                } else {
                    json!([{"type":"emoji","emoji":emoji}])
                },
            );
            Some(request("setMessageReaction", fields))
        }
        ChannelEffect::Edit { message_id, body } => {
            body.validate()?;
            fields.insert("message_id".to_owned(), json!(message_id));
            fields.insert("text".to_owned(), json!(body.text));
            Some(request("editMessageText", fields))
        }
        ChannelEffect::Delete { message_id } | ChannelEffect::Redact { message_id, .. } => {
            fields.insert("message_id".to_owned(), json!(message_id));
            Some(request("deleteMessage", fields))
        }
        ChannelEffect::Pin { message_id } => {
            fields.insert("message_id".to_owned(), json!(message_id));
            Some(request("pinChatMessage", fields))
        }
        ChannelEffect::Unpin { message_id } => {
            fields.insert("message_id".to_owned(), json!(message_id));
            Some(request("unpinChatMessage", fields))
        }
        ChannelEffect::Typing { active: false }
        | ChannelEffect::Preview { .. }
        | ChannelEffect::MarkRead { .. } => None,
    };
    Ok(request)
}

fn request(path: &str, fields: Map<String, serde_json::Value>) -> OutboundRequest {
    OutboundRequest {
        method: "POST".to_owned(),
        path: path.to_owned(),
        content_type: "application/json".to_owned(),
        body: serde_json::Value::Object(fields).to_string(),
        headers: std::collections::BTreeMap::new(),
    }
}
