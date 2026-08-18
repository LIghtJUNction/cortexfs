use std::collections::BTreeMap;

use serde_json::json;

use super::{ChannelCodec, OutboundRequest, object};
use crate::{ChannelError, ChannelId, OutboundMessage};

mod parse;

/// Notion database-page task codec.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotionCodec {
    status_property: String,
    input_property: String,
    result_property: String,
    status_type: String,
}

impl Default for NotionCodec {
    fn default() -> Self {
        Self::new("Status", "Input", "Result")
    }
}

impl NotionCodec {
    #[must_use]
    pub fn new(
        status_property: impl Into<String>,
        input_property: impl Into<String>,
        result_property: impl Into<String>,
    ) -> Self {
        Self {
            status_property: status_property.into(),
            input_property: input_property.into(),
            result_property: result_property.into(),
            status_type: "select".to_owned(),
        }
    }

    #[must_use]
    pub fn with_status_type(mut self, status_type: impl Into<String>) -> Self {
        self.status_type = status_type.into();
        self
    }

    #[must_use]
    pub fn status_type(&self) -> &str {
        &self.status_type
    }
}

impl ChannelCodec for NotionCodec {
    fn channel(&self) -> ChannelId {
        ChannelId::from_static("notion")
    }

    fn decode(&self, payload: &str) -> Result<Option<crate::InboundMessage>, ChannelError> {
        parse::decode(&object(payload)?, self)
    }

    fn decode_many(&self, payload: &str) -> Result<Vec<crate::InboundMessage>, ChannelError> {
        let root = object(payload)?;
        if let Some(results) = root.get("results").and_then(serde_json::Value::as_array) {
            return results
                .iter()
                .map(|page| parse::decode(page, self))
                .collect::<Result<Vec<_>, _>>()
                .map(|items| items.into_iter().flatten().collect());
        }
        Ok(parse::decode(&root, self)?.into_iter().collect())
    }

    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError> {
        message.body.validate()?;
        if !message.body.attachments.is_empty() {
            return Err(ChannelError::Unsupported(
                "notion media attachments".to_owned(),
            ));
        }
        let page_id = message.target.conversation.as_str();
        if page_id.is_empty()
            || page_id
                .bytes()
                .any(|byte| matches!(byte, b'/' | b'?' | b'#' | 0))
        {
            return Err(ChannelError::InvalidMessage(
                "notion page id is not a safe path segment".to_owned(),
            ));
        }
        let status = if self.status_type == "status" {
            "status"
        } else {
            "select"
        };
        let mut properties = BTreeMap::new();
        properties.insert(
            self.status_property.clone(),
            json!({status: {"name": "done"}}),
        );
        properties.insert(
            self.result_property.clone(),
            json!({"rich_text": [{"text": {"content": truncate(&message.body.text)}}]}),
        );
        Ok(OutboundRequest {
            method: "PATCH".to_owned(),
            path: format!("pages/{page_id}"),
            content_type: "application/json".to_owned(),
            body: json!({"properties": properties}).to_string(),
            headers: BTreeMap::from([(String::from("Notion-Version"), String::from("2022-06-28"))]),
        })
    }
}

fn truncate(value: &str) -> String {
    let mut output: String = value.chars().take(1_970).collect();
    if output.len() < value.len() {
        output.push_str("\n\n... [output truncated]");
    }
    output
}
