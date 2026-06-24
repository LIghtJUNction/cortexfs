use std::fs;
use std::io::{self, Write};
use std::process::Command;

use cortexfs_tool_sdk::{
    Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult, ToolSpec, run_tool,
};

#[derive(Debug)]
pub struct FsReadTool;

#[derive(Debug)]
pub struct FsWriteTool;

#[derive(Debug)]
pub struct ShellExecTool;

impl Tool for FsReadTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.read",
            description: "Read a UTF-8 text file from the visible filesystem.",
            input_schema: FS_READ_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let path = invocation
            .string_field("path")
            .unwrap_or_else(|| invocation.input().trim().to_owned());
        if path.is_empty() {
            return Err(ToolError::invalid("missing path"));
        }
        match fs::read_to_string(&path) {
            Ok(content) => output
                .message(&content)
                .map_err(|error| ToolError::new("EIO", error.to_string())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(ToolError::not_found("file not found"))
            }
            Err(_error) => Err(ToolError::denied("read failed")),
        }
    }
}

impl Tool for FsWriteTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.write",
            description: "Write UTF-8 text to a path in the visible filesystem.",
            input_schema: FS_WRITE_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let path = invocation.string_field("path").unwrap_or_default();
        let content = invocation.string_field("content").unwrap_or_default();
        if path.is_empty() {
            return Err(ToolError::invalid("missing path"));
        }
        fs::write(path, content).map_err(|_error| ToolError::denied("write failed"))?;
        output
            .message("written")
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}

impl Tool for ShellExecTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "shell.exec",
            description: "Run one shell command in the tool process environment.",
            input_schema: SHELL_EXEC_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let command = invocation
            .string_field("cmd")
            .unwrap_or_else(|| invocation.input().trim().to_owned());
        if command.is_empty() {
            return Err(ToolError::invalid("missing cmd"));
        }
        let command_output = Command::new("sh")
            .arg("-c")
            .arg(&command)
            .output()
            .map_err(|error| ToolError::new("EIO", format!("cannot run shell command: {error}")))?;
        let mut text = String::from_utf8_lossy(&command_output.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&command_output.stderr));
        output
            .message(&text)
            .map_err(|error| ToolError::new("EIO", error.to_string()))?;
        if command_output.status.success() {
            Ok(())
        } else {
            Err(ToolError::new("EIO", "command failed"))
        }
    }
}

#[must_use]
pub fn core_tool_specs() -> Vec<ToolSpec> {
    vec![FsReadTool.spec(), FsWriteTool.spec(), ShellExecTool.spec()]
}

pub fn run_core_tool(
    name: &str,
    invocation: &ToolInvocation,
    writer: &mut dyn Write,
) -> Result<bool, io::Error> {
    match name {
        "fs.read" => run_tool(&FsReadTool, invocation, writer).map(|()| true),
        "fs.write" => run_tool(&FsWriteTool, invocation, writer).map(|()| true),
        "shell.exec" => run_tool(&ShellExecTool, invocation, writer).map(|()| true),
        _ => Ok(false),
    }
}

const FS_READ_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.read input",
  "description": "Read one UTF-8 text file visible to the tool process.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path"],
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to a UTF-8 text file visible to the tool process."
    }
  }
}"#;

const FS_WRITE_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.write input",
  "description": "Write UTF-8 text to one path visible to the tool process.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path", "content"],
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to write."
    },
    "content": {
      "type": "string",
      "description": "UTF-8 content to write."
    }
  }
}"#;

const SHELL_EXEC_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "shell.exec input",
  "description": "Run one shell command in the tool process environment.",
  "type": "object",
  "additionalProperties": false,
  "required": ["cmd"],
  "properties": {
    "cmd": {
      "type": "string",
      "description": "Command line passed to sh -c."
    }
  }
}"#;

#[cfg(test)]
mod tests {
    use super::{FsReadTool, FsWriteTool, ShellExecTool};
    use cortexfs_tool_sdk::{ToolInvocation, run_tool};
    use std::fs;

    #[test]
    fn fs_read_tool_emits_file_content() {
        let path = std::env::temp_dir().join(format!("cortexfs-fs-read-{}", std::process::id()));
        assert!(fs::write(&path, "visible").is_ok());
        let tool = FsReadTool;
        let invocation = ToolInvocation::new("r1", format!(r#"{{"path":"{}"}}"#, path.display()));
        let mut output = Vec::new();
        assert!(run_tool(&tool, &invocation, &mut output).is_ok());
        let text = String::from_utf8(output).unwrap_or_default();
        assert!(text.contains(r#""tool":"fs.read""#));
        assert!(text.contains(r#""text":"visible""#));
        let _ignored = fs::remove_file(path);
    }

    #[test]
    fn fs_write_tool_writes_file_content() {
        let path = std::env::temp_dir().join(format!("cortexfs-fs-write-{}", std::process::id()));
        let tool = FsWriteTool;
        let invocation = ToolInvocation::new(
            "r1",
            format!(r#"{{"path":"{}","content":"stored"}}"#, path.display()),
        );
        let mut output = Vec::new();
        assert!(run_tool(&tool, &invocation, &mut output).is_ok());
        assert_eq!(fs::read_to_string(&path).unwrap_or_default(), "stored");
        let _ignored = fs::remove_file(path);
    }

    #[test]
    fn shell_exec_tool_returns_stdout() {
        let tool = ShellExecTool;
        let invocation = ToolInvocation::new("r1", r#"{"cmd":"printf shell-ok"}"#);
        let mut output = Vec::new();
        assert!(run_tool(&tool, &invocation, &mut output).is_ok());
        let text = String::from_utf8(output).unwrap_or_default();
        assert!(text.contains(r#""tool":"shell.exec""#));
        assert!(text.contains("shell-ok"));
    }
}
