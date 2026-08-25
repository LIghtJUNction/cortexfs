use crate::atomic::write_text_file_atomic;
use crate::read::read_small_text_file;
use crate::replace::{fs_replace_tool_error, replace_exactly_once};
use cortexfs_tool_sdk::{Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult, ToolSpec};
use std::io::Write;
use std::path::Path;

#[derive(Debug)]
pub struct FsReadTool;

impl Tool for FsReadTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.read",
            description: "Read a UTF-8 text file from the visible filesystem.",
            input_schema: crate::schemas::FS_READ_SCHEMA,
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
        match read_small_text_file(Path::new(&path), crate::MAX_FS_READ_BYTES) {
            Ok(content) => output
                .message(&content)
                .map_err(|error| ToolError::new("EIO", error.to_string())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(ToolError::not_found("file not found"))
            }
            Err(_error) => Err(ToolError::denied("read failed")),
        }
    }
}

#[derive(Debug)]
pub struct FsWriteTool;

impl Tool for FsWriteTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.write",
            description: "Write UTF-8 text to a path in the visible filesystem.",
            input_schema: crate::configschema::FS_WRITE_SCHEMA,
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
        write_text_file_atomic(Path::new(&path), &content)
            .map_err(|_error| ToolError::denied("write failed"))?;
        output
            .message("written")
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}

#[derive(Debug)]
pub struct FsReplaceTool;

impl Tool for FsReplaceTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.replace",
            description: "Replace exactly one UTF-8 text span in a visible file.",
            input_schema: crate::configschema::FS_REPLACE_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let path = invocation.string_field("path").unwrap_or_default();
        let old = invocation.string_field("old").unwrap_or_default();
        let new = invocation.string_field("new").unwrap_or_default();
        if path.is_empty() {
            return Err(ToolError::invalid("missing path"));
        }
        replace_exactly_once(Path::new(&path), &old, &new)
            .map_err(|error| fs_replace_tool_error(&error))?;
        output
            .message("replaced")
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}
