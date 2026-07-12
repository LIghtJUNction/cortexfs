use std::io::Write;

use cortexfs_tool_sdk::{
    Tool, ToolEmitter, ToolInvocation, ToolResult, ToolSpec, cortexfs_tool_main,
};

#[derive(Debug)]
struct FixtureTool;

impl Tool for FixtureTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "example.echo",
            description: "CortexFS Tool SDK integration fixture.",
            input_schema: r#"{"type":"object"}"#,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        output
            .message(&format!("native:{}", invocation.input()))
            .map_err(|error| cortexfs_tool_sdk::ToolError::new("EIO", error.to_string()))
    }
}

cortexfs_tool_main!(FixtureTool);
