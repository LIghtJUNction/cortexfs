use crate::{
    FsListTool, FsReadTool, FsReplaceTool, FsStatTool, FsWriteTool, ShellExecTool, TshConfigTool,
    configcli, ctx_root_from_env, run_fs_list_cli, run_fs_read_cli, run_fs_replace_cli,
    run_fs_stat_cli, run_fs_write_cli, run_shell_exec_cli,
};
use cortexfs_tool_sdk::{Tool, ToolInvocation, ToolSpec, run_tool};
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

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
    ]
}

pub fn run_core_tool(
    name: &str,
    invocation: &ToolInvocation,
    writer: &mut dyn Write,
) -> Result<bool, io::Error> {
    let tool: &dyn Tool = match name {
        "fs.read" => &FsReadTool,
        "fs.list" => &FsListTool,
        "fs.stat" => &FsStatTool,
        "fs.write" => &FsWriteTool,
        "fs.replace" => &FsReplaceTool,
        "shell.exec" => &ShellExecTool,
        "tsh.config" => &TshConfigTool,
        _ => return Ok(false),
    };
    run_tool(tool, invocation, writer).map(|_code| true)
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
    let result = match name {
        "fs.read" => run_fs_read_cli(args, writer),
        "fs.list" => run_fs_list_cli(args, writer),
        "fs.stat" => run_fs_stat_cli(args, writer),
        "fs.write" => run_fs_write_cli(args, writer),
        "fs.replace" => run_fs_replace_cli(args, writer),
        "shell.exec" => run_shell_exec_cli(args, writer),
        "tsh.config" => configcli::run_tsh_config_cli(root, args, writer),
        _ => return Ok(None),
    }?;
    Ok(Some(result))
}
