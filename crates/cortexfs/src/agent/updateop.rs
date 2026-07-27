#![expect(
    clippy::redundant_pub_crate,
    reason = "self-update tool is shared across private agent, runtime, and tool modules"
)]

use cortexfs_tool_sdk::{Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult, ToolSpec};
use std::io::Write;

pub(crate) const AGENT_UPDATE_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "agent.update input",
  "description": "Replace one authority-free prompt control of the calling agent itself.",
  "type": "object",
  "additionalProperties": false,
  "required": ["control", "content"],
  "properties": {
    "control": { "enum": ["system.md", "prompt.template.md"] },
    "content": { "type": "string" }
  }
}"#;

/// Self-iteration tool: replaces one prompt control of the calling agent.
///
/// The write travels through the receipt-bound run capability socket, so the
/// host revalidates the control name, content, and bounds and applies the
/// replacement atomically. Prompt text grants no authority; the update takes
/// effect when the next run renders its prompt.
#[derive(Debug)]
pub(crate) struct AgentUpdateTool;

impl Tool for AgentUpdateTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "agent.update",
            description: "Replace one prompt control of the calling agent itself.",
            input_schema: AGENT_UPDATE_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let value = serde_json::from_str::<serde_json::Value>(invocation.input())
            .map_err(|_error| ToolError::invalid("invalid json input"))?;
        let object = value
            .as_object()
            .ok_or_else(|| ToolError::invalid("input must be a json object"))?;
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "control" | "content"))
        {
            return Err(ToolError::invalid("unknown agent.update field"));
        }
        let control = object
            .get("control")
            .and_then(serde_json::Value::as_str)
            .filter(|control| cortexfs_runtime_client::is_agent_prompt_control(control))
            .ok_or_else(|| ToolError::invalid("control must be system.md or prompt.template.md"))?;
        let content = object
            .get("content")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::invalid("content must be a string"))?;
        if content.len() > cortexfs_runtime_client::MAX_SELF_UPDATE_CONTENT_BYTES {
            return Err(ToolError::invalid("content exceeds the 8 KiB update bound"));
        }
        // Each invocation is a distinct request: a fresh id keeps the run
        // capability's replay dedup per frame without capping updates at one
        // per run or colliding with `agent.create`'s request id.
        let request_id = crate::support::receipt::random_hex::<8>()
            .map(|nonce| format!("update-{nonce}"))
            .map_err(|error| ToolError::new("EIO", error.to_string()))?;
        crate::runtime::control::update_prompt_from_environment(
            crate::runtime::control::UpdatePromptEnvironmentRequest {
                request_id: &request_id,
                control,
                content,
            },
        )
        .map_err(|error| ToolError::new(error.errno(), error.to_string()))?;
        output
            .message(&format!(
                "self {control} updated; applies from the next run"
            ))
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}
