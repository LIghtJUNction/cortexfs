#![forbid(unsafe_code)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "private modules share narrow helpers"
)]

//! Default tool implementations shipped with `CortexFS`.

pub(crate) mod atomic;
pub mod config;
pub(crate) mod configcli;
pub(crate) mod configio;
pub(crate) mod configparse;
pub(crate) mod configschema;
pub(crate) mod dispatch;
pub(crate) mod filecli;
pub mod files;
pub(crate) mod input;
pub mod inspect;
pub(crate) mod inspectcli;
pub(crate) mod plain;
pub(crate) mod read;
pub(crate) mod replace;
pub mod schemas;
pub mod shell;
pub(crate) mod shellerror;
pub(crate) mod wait;
pub(crate) mod waitread;

use cortexfs_tool_sdk::ToolError;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

pub const MAX_FS_READ_BYTES: u64 = 1024 * 1024;
pub const MAX_FS_WRITE_BYTES: usize = 64 * 1024;
pub const MAX_TSH_CONFIG_BYTES: u64 = 64 * 1024;
pub const MAX_TSH_TOOL_COUNT: usize = 1024;
pub const MAX_SHELL_EXEC_OUTPUT_BYTES: usize = 64 * 1024;
pub const SHELL_EXEC_TIMEOUT_SECONDS: u64 = 20;
pub const SHELL_EXEC_SHELL: &str = "/bin/sh";

pub use atomic::write_text_file_atomic;
pub use config::{
    TshRuntimeConfig, default_tsh_config_path, positive_usize, requested_tsh_config_path,
    tsh_tool_count,
};
pub use configcli::TshConfigTool;
pub use configio::{
    create_tsh_config_dir, format_tsh_runtime_config, read_tsh_runtime_config,
    write_tsh_runtime_config,
};
pub use configparse::{TshConfigParseError, parse_tsh_runtime_config};
pub use configschema::{FS_REPLACE_SCHEMA, FS_WRITE_SCHEMA, SHELL_EXEC_SCHEMA, TSH_CONFIG_SCHEMA};
pub use dispatch::{
    core_tool_specs, run_core_tool, run_core_tool_cli, run_core_tool_cli_with_root,
};
pub use filecli::{run_fs_read_cli, run_fs_replace_cli, run_fs_write_cli};
pub use files::{FsReadTool, FsReplaceTool, FsWriteTool};
pub use input::read_text_from_stdin_limited;
pub use inspect::{FsListTool, FsStatTool, MAX_FS_LIST_ENTRIES};
pub use inspectcli::{run_fs_list_cli, run_fs_stat_cli};
pub use replace::{fs_replace_tool_error, replace_exactly_once};
pub use schemas::{FS_LIST_SCHEMA, FS_READ_SCHEMA, FS_STAT_SCHEMA};
pub use shell::{
    ShellExecTool, run_shell_exec_cli, run_shell_exec_command, run_shell_exec_command_with_timeout,
    shell_exec_command,
};
pub use shellerror::ShellExecError;

pub fn ctx_root_from_env() -> PathBuf {
    std::env::var_os("CTX_ROOT")
        .map_or_else(|| PathBuf::from(cortexfs_paths::CTX_ROOT), PathBuf::from)
}

pub(crate) fn exit_code_from_status(status: std::process::ExitStatus) -> ExitCode {
    status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or_else(|| ExitCode::from(1), ExitCode::from)
}

pub(crate) fn tool_error_to_io(error: &ToolError) -> io::Error {
    io::Error::other(format!("{}: {}", error.code(), error.message()))
}
