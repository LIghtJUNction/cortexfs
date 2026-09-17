use crate::{ContentPart, anthropic::Block};

pub(super) fn part(source: &Block<'_>) -> Option<ContentPart> {
    match *source {
        Block::Text { ref text } => Some(ContentPart::text(text.as_ref())),
        Block::Thinking {
            ref thinking,
            ref signature,
        } => Some(ContentPart::Data {
            name: "anthropic.thinking".to_owned(),
            value: serde_json::json!({"text": thinking, "signature": signature}),
        }),
        Block::ToolUse { .. } => None,
        Block::ToolResult {
            ref tool_use_id,
            ref content,
            is_error,
        } => Some(ContentPart::Data {
            name: format!("anthropic.tool_result:{tool_use_id}"),
            value: serde_json::json!({"content": content, "is_error": is_error}),
        }),
    }
}
