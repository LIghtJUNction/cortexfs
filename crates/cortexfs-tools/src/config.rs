use crate::configio::{read_tsh_runtime_config, write_tsh_runtime_config};
use cortexfs_tool_sdk::{ToolError, ToolResult};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TshRuntimeConfig {
    pub max_loaded_tools: usize,
    pub cache_capacity: usize,
    pub window_percent: usize,
}

impl Default for TshRuntimeConfig {
    fn default() -> Self {
        Self {
            max_loaded_tools: 64,
            cache_capacity: 32,
            window_percent: 1,
        }
    }
}

#[must_use]
pub fn default_tsh_config_path(root: &Path) -> PathBuf {
    root.join("tool/tsh.d/config")
}

pub fn requested_tsh_config_path(root: &Path, object: &Map<String, Value>) -> ToolResult<PathBuf> {
    let default_path = default_tsh_config_path(root);
    let Some(value) = object.get("path") else {
        return Ok(default_path);
    };
    let Some(path) = value.as_str() else {
        return Err(ToolError::invalid("path must be a string"));
    };
    (Path::new(path) == default_path.as_path())
        .then_some(default_path)
        .ok_or_else(|| {
            ToolError::denied("tsh.config path is restricted to CTX_ROOT/tool/tsh.d/config")
        })
}

pub(crate) fn apply_request(root: &Path, input: &str) -> ToolResult<(PathBuf, TshRuntimeConfig)> {
    let request = if input.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str::<Value>(input)
            .map_err(|_error| ToolError::invalid("invalid json input"))?
    };
    let Some(object) = request.as_object() else {
        return Err(ToolError::invalid("input must be a json object"));
    };
    let path = requested_tsh_config_path(root, object)?;
    let mut config = read_tsh_runtime_config(&path)?;
    let changed = ["max_loaded_tools", "cache_capacity", "window_percent"]
        .iter()
        .any(|key| object.contains_key(*key));
    if let Some(value) = object.get("max_loaded_tools") {
        config.max_loaded_tools = tsh_tool_count(value, "max_loaded_tools")?;
    }
    if let Some(value) = object.get("cache_capacity") {
        config.cache_capacity = tsh_tool_count(value, "cache_capacity")?;
    }
    if let Some(value) = object.get("window_percent") {
        let value = positive_usize(value, "window_percent")?;
        if !(1..=100).contains(&value) {
            return Err(ToolError::invalid("window_percent must be 1..100"));
        }
        config.window_percent = value;
    }
    if changed {
        write_tsh_runtime_config(&path, config)?;
    }
    Ok((path, config))
}

pub fn positive_usize(value: &Value, field: &str) -> ToolResult<usize> {
    value
        .as_u64()
        .and_then(|value| usize::try_from(value).ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| ToolError::invalid(format!("{field} must be a positive integer")))
}

pub fn tsh_tool_count(value: &Value, field: &str) -> ToolResult<usize> {
    let value = positive_usize(value, field)?;
    (value <= crate::MAX_TSH_TOOL_COUNT)
        .then_some(value)
        .ok_or_else(|| {
            ToolError::invalid(format!("{field} must be 1..{}", crate::MAX_TSH_TOOL_COUNT))
        })
}
