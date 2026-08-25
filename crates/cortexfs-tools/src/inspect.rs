use crate::plain::{open_plain_directory, path_metadata_no_follow, proc_fd_path};
use cortexfs_tool_sdk::{Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult, ToolSpec};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

pub const MAX_FS_LIST_ENTRIES: usize = 256;

#[derive(Debug)]
pub struct FsListTool;
#[derive(Debug)]
pub struct FsStatTool;

impl Tool for FsListTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.list",
            description: "List bounded file metadata in a visible directory.",
            input_schema: crate::schemas::FS_LIST_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let path = request_path(invocation)?;
        let max = request_max_entries(invocation)?;
        let directory = open_plain_directory(&path)
            .map_err(|error| ToolError::denied(format!("directory open failed: {error}")))?;
        let entries = fs::read_dir(proc_fd_path(&directory))
            .map_err(|error| ToolError::denied(format!("directory read failed: {error}")))?;
        let mut values = BTreeMap::new();
        for entry in entries {
            let entry = entry.map_err(|error| {
                ToolError::denied(format!("directory entry read failed: {error}"))
            })?;
            let metadata = entry
                .path()
                .symlink_metadata()
                .map_err(|error| ToolError::denied(format!("entry stat failed: {error}")))?;
            let name = entry.file_name().to_string_lossy().into_owned();
            values.insert(name.clone(), metadata_value(&name, &metadata));
            if values.len() > MAX_FS_LIST_ENTRIES {
                values.pop_last();
            }
        }
        output
            .json_message(&Value::Array(values.into_values().take(max).collect()))
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}

impl Tool for FsStatTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.stat",
            description: "Read bounded no-follow metadata for one visible path.",
            input_schema: crate::schemas::FS_STAT_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let path = request_path(invocation)?;
        let metadata = path_metadata_no_follow(&path)
            .map_err(|error| ToolError::denied(format!("path stat failed: {error}")))?;
        output
            .json_message(&metadata_value(path.to_string_lossy().as_ref(), &metadata))
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}

fn request_path(invocation: &ToolInvocation) -> ToolResult<PathBuf> {
    let path = invocation
        .string_field("path")
        .unwrap_or_else(|| invocation.input().trim().to_owned());
    (!path.is_empty())
        .then_some(PathBuf::from(path))
        .ok_or_else(|| ToolError::invalid("missing path"))
}

fn request_max_entries(invocation: &ToolInvocation) -> ToolResult<usize> {
    let Some(value) = invocation.value_field("max_entries") else {
        return Ok(MAX_FS_LIST_ENTRIES);
    };
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| (1..=MAX_FS_LIST_ENTRIES).contains(value))
        .ok_or_else(|| ToolError::invalid("max_entries must be 1..256"))
}

pub(crate) fn metadata_value(name: &str, metadata: &fs::Metadata) -> Value {
    let kind = if metadata.is_dir() {
        "directory"
    } else if metadata.is_file() {
        "file"
    } else if metadata.file_type().is_symlink() {
        "symlink"
    } else {
        "other"
    };
    json!({ "name": name, "type": kind, "size": metadata.len(), "mode": metadata.mode() & 0o7777 })
}
