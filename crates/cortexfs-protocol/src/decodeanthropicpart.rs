use crate::{ContentPart, anthropic::Block};

pub(super) fn part(source: &Block<'_>) -> Option<ContentPart> {
    match source {
        Block::Text { text } => Some(ContentPart::text(text.as_ref())),
        Block::Thinking { thinking, signature } => Some(ContentPart::Data {
            name: "anthropic.thinking".to_owned(),
            value: serde_json::json!({"text": thinking, "signature": signature}),
        }),
        Block::ToolUse { .. } => None,
        Block::ToolResult { tool_use_id, content, is_error } => Some(ContentPart::Data {
            name: format!("anthropic.tool_result:{tool_use_id}"),
            value: serde_json::json!({"content": content, "is_error": is_error}),
        }),
    }
}
