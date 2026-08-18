use std::collections::BTreeMap;

use serde_json::json;

use super::super::OutboundRequest;
use crate::{ChannelError, OutboundMessage};

pub(super) fn request(message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
    message.body.validate()?;
    if message.body.attachments.len() > 1 {
        return Err(ChannelError::Unsupported(
            "line multiple media attachments".to_owned(),
        ));
    }
    let item = message.body.attachments.first().map_or_else(
        || json!({"type":"text","text":message.body.text}),
        |attachment| {
            if attachment
                .mime
                .as_deref()
                .is_some_and(|mime| mime.starts_with("image/"))
            {
                json!({"type":"image","originalContentUrl":attachment.url,"previewImageUrl":attachment.url})
            } else {
                json!({"type":"text","text":message.body.text})
            }
        },
    );
    let (path, body) = message.metadata.get("line.reply_token").map_or_else(
        || {
            (
                "v2/bot/message/push",
                json!({"to": message.target.conversation.as_str(), "messages": [item.clone()]}),
            )
        },
        |token| {
            (
                "v2/bot/message/reply",
                json!({"replyToken": token, "messages": [item]}),
            )
        },
    );
    Ok(OutboundRequest {
        method: "POST".to_owned(),
        path: path.to_owned(),
        content_type: "application/json".to_owned(),
        body: body.to_string(),
        headers: BTreeMap::new(),
    })
}
