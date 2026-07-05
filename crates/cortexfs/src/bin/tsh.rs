#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::fmt::Write as FmtWrite;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode, Stdio};
use std::{env, fs};

use cortexfs::{
    AgentRuntimeViewError, CTX_ROOT, PolicyV0, ToolExecutionAuthority, ToolExecutionDenial,
    ToolPath, authorize_tool_execution, derive_agent_runtime_view,
};
use nix::libc;
use nix::sys::termios::{self, ControlFlags, InputFlags, LocalFlags, OutputFlags, SetArg};
use serde_json::Value;

include!("shared/stderr.rs");
include!("shared/current_uid.rs");
include!("shared/shell_words.rs");
include!("shared/simple_cli_error.rs");

define_simple_cli_error!(TshError);

const MAX_TSH_CONTROL_BYTES: u64 = 64 * 1024;
const MAX_TSH_REPL_LINE_BYTES: usize = 1024 * 1024;
const MAX_TSH_TOOL_COUNT: usize = 1024;
type TshToolEnv = Vec<(String, String)>;
type AuthorizedTshTool = (cortexfs::ToolExecutionGrant, TshToolEnv);

fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(code) => code,
        Err(error) => {
            let _ignored = write_error(&format!("tsh: {}", error.message));
            ExitCode::from(error.code)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<ExitCode, TshError> {
    let (root, command) = parse_args(args)?;
    match command {
        TshCommand::Help => return print_help().map(|()| ExitCode::SUCCESS),
        TshCommand::List => return list_tools(&root).map(|()| ExitCode::SUCCESS),
        TshCommand::Repl | TshCommand::Tool { .. } => {}
    }
    let config = read_tsh_config(&root)?;
    let mut cache =
        DynamicToolCache::with_window_percent(config.cache_capacity, config.window_percent);
    let mut context = ToolContext::new(config.max_loaded_tools);
    match command {
        TshCommand::Repl => run_repl(&root, &mut cache, &mut context),
        TshCommand::Tool { name, args } if is_tsh_builtin(&name) => {
            run_builtin_once(&root, &mut cache, &mut context, &name, args)
        }
        TshCommand::Tool { name, args } => run_tool(&root, &name, args),
        TshCommand::Help => print_help().map(|()| ExitCode::SUCCESS),
        TshCommand::List => list_tools(&root).map(|()| ExitCode::SUCCESS),
    }
}

#[derive(Debug, Eq, PartialEq)]
enum TshCommand {
    Help,
    List,
    Repl,
    Tool { name: String, args: Vec<OsString> },
}

fn parse_args(args: Vec<OsString>) -> Result<(PathBuf, TshCommand), TshError> {
    let mut root = env::var_os("CTX_ROOT").map_or_else(|| PathBuf::from(CTX_ROOT), PathBuf::from);
    let mut values = args.into_iter();
    let mut rest = Vec::new();

    while let Some(value) = values.next() {
        let text = os_string(value)?;
        match text.as_str() {
            "--root" | "-r" => {
                let Some(path) = values.next() else {
                    return Err(TshError::usage("--root requires a path"));
                };
                root = PathBuf::from(path);
            }
            "--help" | "-h" => return Ok((root, TshCommand::Help)),
            "--list" => return Ok((root, TshCommand::List)),
            _ => {
                rest.push(OsString::from(text));
                rest.extend(values);
                break;
            }
        }
    }

    if rest.is_empty() {
        return Ok((root, TshCommand::Repl));
    }
    let mut command = rest.into_iter();
    let Some(name) = command.next() else {
        return Ok((root, TshCommand::Repl));
    };
    Ok((
        root,
        TshCommand::Tool {
            name: os_string(name)?,
            args: command.collect(),
        },
    ))
}

fn os_string(value: OsString) -> Result<String, TshError> {
    value.into_string().map_err(|value| {
        TshError::usage(format!(
            "arguments must be valid UTF-8: {}",
            value.to_string_lossy()
        ))
    })
}

fn print_help() -> Result<(), TshError> {
    write_stdout(help_text())
}

fn help_text() -> &'static str {
    "\
tsh - CortexFS tool shell

usage:
  tsh [--root PATH] [--list]
  tsh [--root PATH] TOOL [ARG...]

principles:
  tsh resolves TOOL through CTX_PATH
  standalone tsh reads CTX_HOME/.tshrc before inherited CTX_PATH
  tsh never falls back to PATH for tool lookup
  bash, tmux, and zellij are tools, not built-ins

repl:
  help             show this help
  tools            list visible tools
  tools -l         list visible tools with paths and descriptions
  which TOOL       print the resolved tool path
  type TOOL        explain whether TOOL is a builtin or visible tool
  command -v TOOL  print the command that tsh would run
  help TOOL        show metadata for a visible tool
  load TOOL        load tool metadata into this tsh context
  unload TOOL      remove unpinned tool metadata from this tsh context
  loads            list loaded tool context entries
  pin TOOL         load TOOL metadata and keep it from context eviction
  unpin TOOL       allow a pinned tool to be unloaded from context again
  pins             list pinned tool context entries
  TOOL [ARG...]    run a visible tool with CLI-style argv and stdio
  bash             enter an interactive shell tool
  exit             leave tsh
"
}

fn list_tools(root: &Path) -> Result<(), TshError> {
    list_tools_with_mode(root, ToolListMode::Names)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ToolListMode {
    Names,
    Long,
}

fn list_tools_with_mode(root: &Path, mode: ToolListMode) -> Result<(), TshError> {
    let tool_path = ctx_tool_path(root)?;
    let hits = tool_path.list().map_err(tool_path_error)?;
    let mut stdout = io::stdout().lock();
    for hit in hits {
        let Some(name) = hit.path().file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        match mode {
            ToolListMode::Names => {
                writeln!(stdout, "{name}").map_err(|error| write_error_to_tsh(&error))?;
            }
            ToolListMode::Long => {
                let description = tool_description(&hit);
                if description.is_empty() {
                    writeln!(stdout, "{name}\t{}", hit.path().display())
                        .map_err(|error| write_error_to_tsh(&error))?;
                } else {
                    writeln!(stdout, "{name}\t{}\t{description}", hit.path().display())
                        .map_err(|error| write_error_to_tsh(&error))?;
                }
            }
        }
    }
    stdout.flush().map_err(|error| write_error_to_tsh(&error))
}

fn read_tsh_config(root: &Path) -> Result<cortexfs::tool::core::tools::TshRuntimeConfig, TshError> {
    let mut config = if let Some(content) = read_tsh_config_text(root)? {
        cortexfs::tool::core::tools::parse_tsh_runtime_config(&content)
            .map_err(|message| TshError::usage(format!("invalid tsh.d/config: {message}")))?
    } else {
        cortexfs::tool::core::tools::TshRuntimeConfig::default()
    };
    if let Some(capacity) = env::var("CTX_TOOL_CACHE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
    {
        config.cache_capacity = capacity.clamp(1, MAX_TSH_TOOL_COUNT);
    }
    Ok(config)
}

fn read_tsh_config_text(root: &Path) -> Result<Option<String>, TshError> {
    let tool_path = ctx_tool_path(root)?;
    let Some(hit) = tool_path.find("tsh").map_err(tool_path_error)? else {
        return Ok(None);
    };
    let path = hit.control_dir().join("config");
    match read_small_plain_text_file(&path, MAX_TSH_CONTROL_BYTES, "tsh") {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(TshError::unavailable(format!(
            "cannot read {}: {error}",
            path.display()
        ))),
    }
}

include!("tsh/context.rs");

include!("tsh/terminal.rs");

include!("tsh/repl_parse.rs");
include!("tsh/repl.rs");
include!("shared/plain_dir.rs");
include!("shared/proc_fd.rs");
include!("shared/no_follow_fs.rs");
include!("shared/small_text.rs");

fn run_tool(root: &Path, name: &str, args: Vec<OsString>) -> Result<ExitCode, TshError> {
    let (grant, env) = authorize_tsh_tool_execution(root, name)?;
    let hit = grant.hit();
    if args.len() == 1
        && matches!(
            args.first().and_then(|arg| arg.to_str()),
            Some("-h" | "--help")
        )
    {
        return print_tool_help(root, name).map(|()| ExitCode::SUCCESS);
    }
    let tool_executable = open_executable_no_follow(hit.path())
        .map_err(|error| TshError::unavailable(format!("cannot run tool: {error}")))?;
    let status = ProcessCommand::new(proc_fd_path(&tool_executable))
        .args(args)
        .env_clear()
        .envs(env.iter().map(|env| (env.0.as_str(), env.1.as_str())))
        .env("CTX_ROOT", root)
        .env("CTX_AGENT", agent_name_from_env()?)
        .env("CTX_TOOL_MODE", "cli")
        .env("CTX_AUTHORIZED_OBJECT", format!("/ctx/tool/{name}"))
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| TshError::unavailable(format!("cannot run tool: {error}")))?;
    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or_else(|| ExitCode::from(1), ExitCode::from))
}

fn authorize_tsh_tool_execution(root: &Path, name: &str) -> Result<AuthorizedTshTool, TshError> {
    let agent_name = agent_name_from_env()?;
    let view = derive_agent_runtime_view(root, &agent_name)
        .map_err(|error| agent_view_error_to_tsh(&error))?;
    let Some(view_hit) = view.tool_path().find(name).map_err(tool_path_error)? else {
        return command_not_found(name);
    };
    let policy_path = view_hit.control_dir().join("policy");
    let policy_text = read_small_plain_text_file(&policy_path, MAX_TSH_CONTROL_BYTES, "tsh")
        .map_err(|error| {
            TshError::unavailable(format!("cannot read {}: {error}", policy_path.display()))
        })?;
    let tool_policy = PolicyV0::parse(&policy_text)
        .map_err(|_error| TshError::unavailable(format!("invalid policy for tool:{name}")))?;
    let grant = authorize_tool_execution(
        view.tool_path(),
        name,
        ToolExecutionAuthority::new(
            view.identity(),
            view.mount_table(),
            view.policy_subject(),
            view.policy(),
            &tool_policy,
        ),
    )
    .map_err(|denial| tool_execution_denial_to_tsh(name, denial))?;
    Ok((grant, view.env().to_vec()))
}

fn agent_name_from_env() -> Result<String, TshError> {
    env::var("CTX_AGENT").map_err(|error| match error {
        env::VarError::NotPresent => TshError::unavailable(
            "cannot authorize tool execution: CTX_AGENT is not set; use `ctx agent attach AGENT` to run tools in an agent terminal",
        ),
        env::VarError::NotUnicode(_value) => TshError::usage("CTX_AGENT must be UTF-8"),
    })
}

fn agent_view_error_to_tsh(error: &AgentRuntimeViewError) -> TshError {
    TshError::unavailable(format!("cannot derive agent authority: {}", error.errno()))
}

fn tool_execution_denial_to_tsh(name: &str, denial: ToolExecutionDenial) -> TshError {
    TshError::unavailable(format!("cannot execute tool:{name}: {}", denial.errno()))
}

include!("tsh/tool_lookup.rs");

fn write_stdout(message: &str) -> Result<(), TshError> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(message.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|error| write_error_to_tsh(&error))
}

fn report_repl_error(error: &TshError) -> Result<(), TshError> {
    write_error(&format!("tsh: {}", error.message)).map_err(|error| write_error_to_tsh(&error))
}

fn write_error_to_tsh(error: &io::Error) -> TshError {
    TshError::unavailable(format!("cannot write output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        DynamicToolCache, LoadedTool, MAX_TSH_REPL_LINE_BYTES, ToolContext, TshCommand,
        append_schema_help, builtin_words, ctx_tool_path_with_home, help_text, load_tool_context,
        open_executable_no_follow, parse_args, parse_repl_line, parse_tshrc_ctx_path,
        read_repl_line_canonical_from, read_tsh_config_text, requires_explicit_repl_input,
        run_repl_tool, run_tool, terminal_safe_text, tiny_lfu_admits, tshrc_ctx_path,
        validate_tshrc_ctx_path, wtinylfu_victim_path,
    };
    use cortexfs::tool::core::tools::{TshRuntimeConfig, parse_tsh_runtime_config};
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::fs::symlink;
    use std::path::Path;
    use std::path::PathBuf;
    use std::process::ExitCode;

    include!("tsh/tests/argument_path.rs");
    include!("tsh/tests/context_cache.rs");
    include!("tsh/tests/execution.rs");
}
