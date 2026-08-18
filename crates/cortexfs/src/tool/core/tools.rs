use crate::CTX_ROOT;
use crate::agent::createop::AgentCreateTool;
use crate::agent::updateop::AgentUpdateTool;
use cortexfs_tool_sdk::{
    Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult, ToolSpec, run_tool,
};
use serde_json::Map;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const SHELL_EXEC_SHELL: &str = crate::support::command::SH;
const MAX_FS_READ_BYTES: u64 = 1024 * 1024;
const MAX_TSH_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_TSH_TOOL_COUNT: usize = 1024;
const MAX_SHELL_EXEC_OUTPUT_BYTES: usize = 64 * 1024;
const SHELL_EXEC_TIMEOUT_SECONDS: u64 = 20;

#[derive(Debug)]
pub struct FsReadTool;

#[derive(Debug)]
pub struct FsWriteTool;

#[derive(Debug)]
pub struct FsReplaceTool;

#[derive(Debug)]
pub struct ShellExecTool;

#[derive(Debug)]
pub struct TshConfigTool;

pub mod config;
pub mod files;
pub mod inspect;
pub mod schemas;
pub mod shell;

pub use config::*;
pub(crate) use files::*;
pub(crate) use inspect::{FsListTool, FsStatTool, run_fs_list_cli, run_fs_stat_cli};
pub(crate) use schemas::*;
pub(crate) use shell::*;

#[must_use]
pub fn core_tool_specs() -> Vec<ToolSpec> {
    vec![
        FsReadTool.spec(),
        FsListTool.spec(),
        FsStatTool.spec(),
        FsWriteTool.spec(),
        FsReplaceTool.spec(),
        ShellExecTool.spec(),
        TshConfigTool.spec(),
        AgentCreateTool.spec(),
        AgentUpdateTool.spec(),
    ]
}

pub fn run_core_tool(
    name: &str,
    invocation: &ToolInvocation,
    writer: &mut dyn Write,
) -> Result<bool, io::Error> {
    match name {
        "fs.read" => run_tool(&FsReadTool, invocation, writer).map(|_code| true),
        "fs.list" => run_tool(&FsListTool, invocation, writer).map(|_code| true),
        "fs.stat" => run_tool(&FsStatTool, invocation, writer).map(|_code| true),
        "fs.write" => run_tool(&FsWriteTool, invocation, writer).map(|_code| true),
        "fs.replace" => run_tool(&FsReplaceTool, invocation, writer).map(|_code| true),
        "shell.exec" => run_tool(&ShellExecTool, invocation, writer).map(|_code| true),
        "tsh.config" => run_tool(&TshConfigTool, invocation, writer).map(|_code| true),
        "agent.create" => run_tool(&AgentCreateTool, invocation, writer).map(|_code| true),
        "agent.update" => run_tool(&AgentUpdateTool, invocation, writer).map(|_code| true),
        _ => Ok(false),
    }
}

pub fn run_core_tool_cli(
    name: &str,
    args: &[OsString],
    writer: &mut dyn Write,
) -> Result<Option<ExitCode>, io::Error> {
    run_core_tool_cli_with_root(&ctx_root_from_env(), name, args, writer)
}

pub fn run_core_tool_cli_with_root(
    root: &Path,
    name: &str,
    args: &[OsString],
    writer: &mut dyn Write,
) -> Result<Option<ExitCode>, io::Error> {
    match name {
        "fs.read" => run_fs_read_cli(args, writer).map(Some),
        "fs.list" => run_fs_list_cli(args, writer).map(Some),
        "fs.stat" => run_fs_stat_cli(args, writer).map(Some),
        "fs.write" => run_fs_write_cli(args, writer).map(Some),
        "fs.replace" => run_fs_replace_cli(args, writer).map(Some),
        "shell.exec" => run_shell_exec_cli(args, writer).map(Some),
        "tsh.config" => run_tsh_config_cli(root, args, writer).map(Some),
        "agent.create" => {
            let invocation = cli_invocation(args);
            run_tool(&AgentCreateTool, &invocation, writer).map(Some)
        }
        "agent.update" => {
            let invocation = cli_invocation(args);
            run_tool(&AgentUpdateTool, &invocation, writer).map(Some)
        }
        _ => Ok(None),
    }
}

fn cli_invocation(args: &[OsString]) -> ToolInvocation {
    let input = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let run = std::env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    ToolInvocation::new(run, input)
}

pub(crate) fn ctx_root_from_env() -> PathBuf {
    std::env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(CTX_ROOT), PathBuf::from)
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

#[cfg(test)]
#[expect(unused_qualifications, reason = "tests use qualified paths")]
pub mod tests;
