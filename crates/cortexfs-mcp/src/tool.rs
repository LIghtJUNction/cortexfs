use crate::{client::Client, config};
use cortexfs_tool_sdk::{Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult, ToolSpec};
use serde::Deserialize;
use serde_json::Value;
use std::env;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Locator {
    transport: String,
    config: PathBuf,
    sha256: String,
    server: String,
    tool: String,
}

#[derive(Debug)]
struct McpTool {
    name: &'static str,
    locator: Locator,
}

#[derive(Debug, Deserialize)]
struct CallToolResult {
    content: Vec<Value>,
    #[serde(rename = "isError", default)]
    is_error: bool,
}

impl Tool for McpTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: self.name,
            description: "Projected MCP tool.",
            input_schema: r#"{"type":"object"}"#,
        }
    }
    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let arguments = invocation.json()?;
        let server = config::read(
            &self.locator.config,
            &self.locator.server,
            Some(&self.locator.sha256),
        )
        .map_err(|error| ToolError::new("EIO", format!("cannot read MCP config: {error}")))?;
        let mut client = Client::start(&server)
            .map_err(|error| ToolError::new("EIO", format!("cannot start MCP server: {error}")))?;
        let result = client
            .call(&self.locator.tool, &arguments)
            .map_err(|error| ToolError::new("EIO", format!("MCP call failed: {error}")))?;
        emit_call_result(result, output)
    }
}

fn emit_call_result(result: Value, output: &mut ToolEmitter<&mut dyn Write>) -> ToolResult<()> {
    let result: CallToolResult = serde_json::from_value(result).map_err(|error| {
        ToolError::new("EIO", format!("invalid MCP tools/call result: {error}"))
    })?;
    output
        .content(&result.content)
        .map_err(|error| ToolError::new("EIO", error.to_string()))?;
    if result.is_error {
        return Err(ToolError::new("EIO", "remote MCP tool returned an error"));
    }
    Ok(())
}

pub(crate) fn run() -> io::Result<std::process::ExitCode> {
    let authorized = env::var_os("CTX_AUTHORIZED_OBJECT")
        .map(PathBuf::from)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::PermissionDenied, "missing authorized object")
        })?;
    if !authorized.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "authorized object must be absolute",
        ));
    }
    let name = authorized
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid authorized object"))?
        .to_owned();
    if !cortexfs::is_object_name(&name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid authorized object",
        ));
    }
    let executable = cortexfs::support::plain::open_plain_file(&authorized)?;
    let metadata = executable.metadata()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "authorized object is not executable",
        ));
    }
    let control = authorized.with_file_name(format!("{name}.d"));
    let locator_text =
        cortexfs::support::plain::read_small_text_file(&control.join("mcp"), 64 * 1024)?;
    let locator: Locator = serde_json::from_str(&locator_text)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if locator.transport != "stdio"
        || !locator.config.is_absolute()
        || !valid_digest(&locator.sha256)
        || !cortexfs::is_object_name(&locator.server)
        || !cortexfs::is_object_name(&locator.tool)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid MCP locator",
        ));
    }
    let tool = McpTool {
        name: Box::leak(name.into_boxed_str()),
        locator,
    };
    Ok(cortexfs_tool_sdk::run_cli(&tool, env::args_os().skip(1)))
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::emit_call_result;
    use cortexfs_tool_sdk::{ToolEmitter, ToolResult, parse_jsonl_frames};
    use serde_json::{Value, json};
    use std::io::{self, Write};

    fn emit(value: Value) -> io::Result<(ToolResult<()>, Vec<Value>)> {
        let mut bytes = Vec::new();
        let result = {
            let writer: &mut dyn Write = &mut bytes;
            let mut output = ToolEmitter::new("r-test", writer);
            emit_call_result(value, &mut output)
        };
        let text = String::from_utf8(bytes)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let frames = parse_jsonl_frames(&text)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok((result, frames))
    }

    #[test]
    fn call_result_emits_text_content_directly() -> io::Result<()> {
        let (result, frames) = emit(json!({
            "content": [{"type":"text","text":"ok"}],
            "isError": false
        }))?;

        assert_eq!(
            (result, frames),
            (
                Ok(()),
                vec![json!({
                    "type": "message",
                    "run": "r-test",
                    "role": "tool",
                    "content": [{"type":"text","text":"ok"}]
                })]
            )
        );
        Ok(())
    }

    #[test]
    fn call_result_emits_content_before_remote_error() -> io::Result<()> {
        let (result, frames) = emit(json!({
            "content": [{"type":"text","text":"failed detail"}],
            "isError": true
        }))?;

        assert!(
            matches!(
                result,
                Err(ref error)
                    if error.code() == "EIO"
                        && error.message() == "remote MCP tool returned an error"
            ) && frames
                == vec![json!({
                    "type": "message",
                    "run": "r-test",
                    "role": "tool",
                    "content": [{"type":"text","text":"failed detail"}]
                })]
        );
        Ok(())
    }

    #[test]
    fn call_result_requires_content_array() -> io::Result<()> {
        let (result, frames) = emit(json!({"isError": false}))?;

        assert!(
            matches!(
                result,
                Err(ref error)
                    if error.code() == "EIO"
                        && error.message().contains("missing field `content`")
            ) && frames.is_empty()
        );
        Ok(())
    }
}
