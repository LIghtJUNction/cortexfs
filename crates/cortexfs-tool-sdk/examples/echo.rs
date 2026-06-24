use std::io::Write;

use cortexfs_tool_sdk::{
    Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult, ToolSpec, cortexfs_tool_artifact,
};

#[derive(Debug)]
struct EchoTool;

impl Tool for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "project.echo",
            description: "Echo text for CortexFS tool SDK examples.",
            input_schema: r#"{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}"#,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let text = invocation
            .string_field("text")
            .unwrap_or_else(|| invocation.input().trim().to_owned());
        if text.is_empty() {
            return Err(ToolError::invalid("missing text"));
        }
        output
            .message(&text)
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}

cortexfs_tool_artifact!(
    EchoTool,
    name: "project.echo",
    description: "Echo text for CortexFS tool SDK examples.",
    schema: r#"{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}"#,
);
