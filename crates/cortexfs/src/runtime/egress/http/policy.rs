use serde_json::Value;

use super::invalid;
use crate::provider::openai_response_item_requires_continuation;

pub(super) fn reject_programmatic_tools(endpoint: &str, body: &[u8]) -> std::io::Result<()> {
    if endpoint != "responses" {
        return Ok(());
    }
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return Ok(());
    };
    let tools = value
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| tools.iter().any(is_programmatic_tool));
    let input = value
        .get("input")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(openai_response_item_requires_continuation));
    if tools || input {
        Err(invalid("programmatic tool calling is unsupported"))
    } else {
        Ok(())
    }
}

fn is_programmatic_tool(tool: &Value) -> bool {
    tool.get("type").and_then(Value::as_str) == Some("programmatic_tool_calling")
        || tool.get("allowed_callers").is_some()
        || tool
            .get("function")
            .is_some_and(|function| function.get("allowed_callers").is_some())
}
