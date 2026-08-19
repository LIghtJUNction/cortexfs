use crate::action;
use cortexfs_tool_sdk::{Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult, ToolSpec};
use serde_json::json;
use std::io::Write;

#[derive(Debug)]
pub(crate) struct ChannelTool;

impl Tool for ChannelTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "channel.invoke",
            description: "Request a provider-neutral action from the active channel driver.",
            input_schema: r#"{"type":"object"}"#,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let name = invocation
            .tool_name()
            .ok_or_else(|| ToolError::invalid("missing channel tool name"))?;
        let result = action::run(name, invocation)?;
        output
            .json_message(&json!(result))
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}
