use std::io::Write;

use cortexfs_tool_sdk::{
    Tool, ToolEmitter, ToolInvocation, ToolResult, ToolSpec, cortexfs_tool_main,
};

#[derive(Debug)]
struct EchoTool;

impl Tool for EchoTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "example.echo",
            description: "Echo text through the CortexFS Tool SDK.",
            input_schema: r#"{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}"#,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let text = invocation.required_string_field("text")?;
        output
            .message(&text)
            .map_err(|error| cortexfs_tool_sdk::ToolError::new("EIO", error.to_string()))
    }
}

cortexfs_tool_main!(EchoTool);
