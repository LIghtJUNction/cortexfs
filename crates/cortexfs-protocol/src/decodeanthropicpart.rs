use crate::anthropic::Block;
use crate::{ContentPart, ConversionError};

pub(super) fn part(source: &Block<'_>) -> Result<ContentPart, ConversionError> {
    match *source {
        Block::Text { ref text } => Ok(ContentPart::text(text.as_ref())),
        Block::Thinking {
            ref thinking,
            ref signature,
        } => Ok(ContentPart::Data {
            name: "anthropic.thinking".to_owned(),
            value: serde_json::json!({"text": thinking, "signature": signature}),
        }),
        Block::ToolUse {
            ref id,
            ref name,
            input,
        } => Ok(ContentPart::Data {
            name: format!("anthropic.tool_use:{id}:{name}"),
            value: crate::semantic::raw_value(
                crate::WireProtocol::Anthropic,
                "messages[].content[].input",
                input,
            )?,
        }),
        Block::ToolResult {
            ref tool_use_id,
            ref content,
            is_error,
        } => Ok(ContentPart::Data {
            name: format!("anthropic.tool_result:{tool_use_id}"),
            value: serde_json::json!({"content": content, "is_error": is_error}),
        }),
    }
}
