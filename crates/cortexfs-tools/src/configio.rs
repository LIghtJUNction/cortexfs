use crate::atomic::write_text_file_atomic;
use crate::configparse::parse_tsh_runtime_config;
use crate::read::create_plain_dir;
use crate::{MAX_TSH_CONFIG_BYTES, TshRuntimeConfig};
use cortexfs_tool_sdk::{ToolError, ToolResult};
use std::io;
use std::path::Path;

pub fn read_tsh_runtime_config(path: &Path) -> ToolResult<TshRuntimeConfig> {
    let content = match crate::read::read_small_text_file(path, MAX_TSH_CONFIG_BYTES) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(TshRuntimeConfig::default());
        }
        Err(error) => return Err(ToolError::denied(format!("cannot read config: {error}"))),
    };
    parse_tsh_runtime_config(&content).map_err(|error| ToolError::invalid(error.to_string()))
}

pub fn write_tsh_runtime_config(path: &Path, config: TshRuntimeConfig) -> ToolResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| ToolError::invalid("config path must have a parent directory"))?;
    create_tsh_config_dir(parent)?;
    write_text_file_atomic(path, &format_tsh_runtime_config(config))
        .map_err(|error| ToolError::denied(format!("cannot write config: {error}")))
}

pub fn create_tsh_config_dir(path: &Path) -> ToolResult<()> {
    create_plain_dir(path)
        .map_err(|error| ToolError::denied(format!("cannot create config directory: {error}")))
}

#[must_use]
pub fn format_tsh_runtime_config(config: TshRuntimeConfig) -> String {
    format!(
        "max_loaded_tools={}\ncache_capacity={}\nwindow_percent={}\n",
        config.max_loaded_tools, config.cache_capacity, config.window_percent
    )
}
