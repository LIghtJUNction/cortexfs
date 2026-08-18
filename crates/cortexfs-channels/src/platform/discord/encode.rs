use serde_json::{Map, Value, json};

use crate::{Attachment, ChannelError, OutboundMessage, OutboundRequest};

pub(super) fn request(message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
    message.body.validate()?;
    let mut fields = Map::new();
    fields.insert("content".to_owned(), json!(message.body.text));
    fields.insert("allowed_mentions".to_owned(), json!({"parse": []}));
    if let Some(thread) = message.target.thread.as_deref() {
        fields.insert("thread_id".to_owned(), json!(thread));
    }
    if !message.body.attachments.is_empty() {
        fields.insert(
            "embeds".to_owned(),
            Value::Array(message.body.attachments.iter().map(embed).collect()),
        );
    }
    Ok(OutboundRequest {
        method: "POST".to_owned(),
        path: "webhook".to_owned(),
        content_type: "application/json".to_owned(),
        body: Value::Object(fields).to_string(),
        headers: std::collections::BTreeMap::new(),
    })
}

fn embed(attachment: &Attachment) -> Value {
    let mut fields = Map::new();
    fields.insert(
        "title".to_owned(),
        json!(attachment.name.as_deref().unwrap_or("attachment")),
    );
    if attachment
        .mime
        .as_deref()
        .is_some_and(|mime| mime.starts_with("image/"))
    {
        fields.insert("image".to_owned(), json!({"url": attachment.url}));
    } else {
        fields.insert("url".to_owned(), json!(attachment.url));
    }
    Value::Object(fields)
}
