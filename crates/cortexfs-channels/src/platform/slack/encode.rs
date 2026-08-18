use serde_json::{Value, json};

use crate::{Attachment, ChannelError, OutboundMessage, OutboundRequest};

pub(super) fn request(message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
    message.body.validate()?;
    let fallback = if message.body.text.is_empty() {
        "attachment"
    } else {
        message.body.text.as_str()
    };
    let mut fields = serde_json::Map::new();
    fields.insert(
        "channel".to_owned(),
        json!(message.target.conversation.as_str()),
    );
    fields.insert("text".to_owned(), json!(fallback));
    if let Some(thread) = message.target.thread.as_deref() {
        fields.insert("thread_ts".to_owned(), json!(thread));
    }
    if !message.body.attachments.is_empty() {
        fields.insert("blocks".to_owned(), blocks(message));
    }
    Ok(OutboundRequest {
        method: "POST".to_owned(),
        path: "chat.postMessage".to_owned(),
        content_type: "application/json".to_owned(),
        body: Value::Object(fields).to_string(),
        headers: std::collections::BTreeMap::new(),
    })
}

fn blocks(message: &OutboundMessage) -> Value {
    let mut blocks = Vec::new();
    if !message.body.text.is_empty() {
        blocks.push(json!({
            "type": "section",
            "text": {"type": "mrkdwn", "text": message.body.text}
        }));
    }
    blocks.extend(message.body.attachments.iter().map(attachment_block));
    Value::Array(blocks)
}

fn attachment_block(attachment: &Attachment) -> Value {
    let name = attachment.name.as_deref().unwrap_or("attachment");
    if attachment
        .mime
        .as_deref()
        .is_some_and(|mime| mime.starts_with("image/"))
    {
        json!({
            "type": "image",
            "image_url": attachment.url,
            "alt_text": name
        })
    } else {
        json!({
            "type": "section",
            "text": {"type": "plain_text", "text": format!("{name}: {}", attachment.url)}
        })
    }
}
