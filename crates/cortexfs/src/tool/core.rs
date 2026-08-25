pub mod runtime;

use crate::agent::createop::AgentCreateTool;
use crate::agent::updateop::AgentUpdateTool;
use cortexfs_tool_sdk::{Tool, ToolInvocation, ToolSpec, run_tool};
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

#[must_use]
pub fn core_tool_specs() -> Vec<ToolSpec> {
    let mut specs = cortexfs_tools::core_tool_specs();
    specs.extend([AgentCreateTool.spec(), AgentUpdateTool.spec()]);
    specs
}

pub fn run_core_tool(
    name: &str,
    invocation: &ToolInvocation,
    writer: &mut dyn Write,
) -> Result<bool, io::Error> {
    if cortexfs_tools::run_core_tool(name, invocation, writer)? {
        return Ok(true);
    }
    let tool: &dyn Tool = match name {
        "agent.create" => &AgentCreateTool,
        "agent.update" => &AgentUpdateTool,
        _ => return Ok(false),
    };
    run_tool(tool, invocation, writer).map(|_code| true)
}

pub fn run_core_tool_cli(
    name: &str,
    args: &[OsString],
    writer: &mut dyn Write,
) -> Result<Option<ExitCode>, io::Error> {
    run_core_tool_cli_with_root(&cortexfs_tools::ctx_root_from_env(), name, args, writer)
}

pub fn run_core_tool_cli_with_root(
    root: &Path,
    name: &str,
    args: &[OsString],
    writer: &mut dyn Write,
) -> Result<Option<ExitCode>, io::Error> {
    if let Some(code) = cortexfs_tools::run_core_tool_cli_with_root(root, name, args, writer)? {
        return Ok(Some(code));
    }
    let invocation = agent_cli_invocation(args);
    match name {
        "agent.create" => run_tool(&AgentCreateTool, &invocation, writer).map(Some),
        "agent.update" => run_tool(&AgentUpdateTool, &invocation, writer).map(Some),
        _ => Ok(None),
    }
}

fn agent_cli_invocation(args: &[OsString]) -> ToolInvocation {
    let input = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    let run = std::env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    ToolInvocation::new(run, input)
}

#[cfg(test)]
#[expect(
    unused_imports,
    unused_qualifications,
    reason = "legacy flat tool tests import through their parent module"
)]
mod tools {
    pub(super) use super::{
        core_tool_specs, run_core_tool, run_core_tool_cli, run_core_tool_cli_with_root,
    };
    pub(super) use cortexfs_tools::{
        FsReadTool, FsReplaceTool, FsWriteTool, MAX_SHELL_EXEC_OUTPUT_BYTES, SHELL_EXEC_SHELL,
        ShellExecTool, TshConfigTool, TshRuntimeConfig, parse_tsh_runtime_config,
        read_text_from_stdin_limited, read_tsh_runtime_config, run_shell_exec_command_with_timeout,
        shell_exec_command, tsh_tool_count, write_tsh_runtime_config,
    };

    mod tests;
}
