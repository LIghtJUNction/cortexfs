use std::collections::BTreeMap;

use serde_json::Value;

use super::super::{participant, scalar};
use super::NotionCodec;
use crate::{ChannelError, ConversationId, InboundMessage, MessageBody, MessageTarget};

pub(super) fn decode(
    page: &Value,
    codec: &NotionCodec,
) -> Result<Option<InboundMessage>, ChannelError> {
    let id = scalar(page.get("id"), "notion page id")?;
    let properties = page
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| ChannelError::Protocol("notion page properties are missing".to_owned()))?;
    if let Some(status) = property_text(properties.get(&codec.status_property))
        && status != "pending"
    {
        return Ok(None);
    }
    let text = property_text(properties.get(&codec.input_property)).unwrap_or_default();
    if text.trim().is_empty() {
        return Ok(None);
    }
    let sender = page
        .get("created_by")
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("notion")
        .to_owned();
    let mut metadata = BTreeMap::new();
    metadata.insert("notion.page_id".to_owned(), id.clone());
    Ok(Some(InboundMessage {
        id: id.clone(),
        target: MessageTarget {
            channel: crate::ChannelId::from_static("notion"),
            conversation: ConversationId::new(id)?,
            thread: None,
            reply_to: None,
        },
        sender: participant(None, sender),
        body: MessageBody::text(text)?,
        timestamp_ms: None,
        metadata,
    }))
}

fn property_text(property: Option<&Value>) -> Option<String> {
    let property = property?;
    let kind = property.get("type").and_then(Value::as_str)?;
    let key = match kind {
        "title" => "title",
        "rich_text" => "rich_text",
        "select" | "status" => {
            return property
                .get(kind)
                .and_then(|value| value.get("name"))
                .and_then(Value::as_str)
                .map(str::to_owned);
        }
        _ => return None,
    };
    property.get(key)?.as_array()?.iter().find_map(|item| {
        item.get("plain_text")
            .or_else(|| item.get("text").and_then(|text| text.get("content")))
            .or_else(|| item.get("name"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}
