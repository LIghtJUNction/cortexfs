use std::io::Write;

use cortexfs_tool_sdk::{
    Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult, ToolSpec, cortexfs_tool_main,
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
        if invocation.input() == "__error__" {
            return Err(ToolError::invalid("fixture failure"));
        }
        output
            .message(&format!("native:{}", invocation.input()))
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}

cortexfs_tool_main!(FixtureTool);
