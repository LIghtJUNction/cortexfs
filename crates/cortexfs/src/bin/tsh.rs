#![forbid(unsafe_code)]
#![expect(
    clippy::allow_attributes,
    reason = "allow target-specific lint exceptions"
)]
#![allow(
    unfulfilled_lint_expectations,
    reason = "expected target-specific lint results"
)]
#![expect(
    clippy::wildcard_imports,
    reason = "uniform submodules with wildcard imports"
)]
#![expect(clippy::redundant_pub_crate, reason = "submodule visibility alignment")]
#![expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "internal structs with scoped fields"
)]
#![expect(clippy::module_inception, reason = "allow submodule self name")]

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsString;
use std::fmt::Write as FmtWrite;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode, Stdio};

use cortexfs::{
    AgentRuntimeViewError, CTX_ROOT, PolicyV0, ToolExecutionAuthority, ToolExecutionDenial,
    ToolPath, authorize_tool_execution, derive_agent_runtime_view,
};
use nix::libc;
use nix::sys::termios::{self, ControlFlags, InputFlags, LocalFlags, OutputFlags, SetArg};
use serde_json::Value;

#[path = "shared/current-uid.rs"]
pub mod current_uid;
#[path = "shared/stderr.rs"]
pub mod stderr;
#[macro_use]
#[path = "shared/shell-words.rs"]
pub mod shell_words;
#[macro_use]
#[path = "shared/simple-cli-error.rs"]
pub mod simple_cli_error;

define_simple_cli_error!(TshError);

const MAX_TSH_CONTROL_BYTES: u64 = 64 * 1024;
const MAX_TSH_REPL_LINE_BYTES: usize = 1024 * 1024;
const MAX_TSH_TOOL_COUNT: usize = 1024;
type TshToolEnv = Vec<(String, String)>;
type AuthorizedTshTool = (cortexfs::ToolExecutionGrant, TshToolEnv);

pub(crate) fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(code) => code,
        Err(error) => {
            write_error(&error.message).ok();
            error.code.into()
        }
    }
}

pub(crate) use context::*;
pub(crate) use current_uid::*;
pub(crate) use no_follow_fs::*;
pub(crate) use proc_fd::*;
pub(crate) use repl::*;
pub(crate) use repl_parse::*;
pub(crate) use small_text::*;
pub(crate) use stderr::*;
pub(crate) use terminal::*;
pub(crate) use tool_lookup::*;

pub(crate) fn run(args: Vec<OsString>) -> Result<ExitCode, TshError> {
    let (root, command) = parse_args(args)?;
    match command {
        TshCommand::Help => return print_help().map(|()| ExitCode::SUCCESS),
        TshCommand::List => return list_tools(&root).map(|()| ExitCode::SUCCESS),
        TshCommand::Repl | TshCommand::Tool { .. } => {}
    }
    let config = read_tsh_config(&root)?;
    let mut cache =
        DynamicToolCache::with_window_percent(config.cache_capacity, config.window_percent);
    let mut context = load_persistent_context(&root, config.max_loaded_tools)?;
    restore_persistent_cache(&mut cache, &context);
    match command {
        TshCommand::Repl => run_repl(&root, &mut cache, &mut context),
        TshCommand::Tool { name, args } if is_tsh_builtin(&name) => {
            run_builtin_once(&root, &mut cache, &mut context, &name, args)
        }
        TshCommand::Tool { name, args } => {
            run_tool_with_context(&root, &mut cache, &mut context, &name, args)
        }
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

pub(crate) fn parse_args(args: Vec<OsString>) -> Result<(PathBuf, TshCommand), TshError> {
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

pub(crate) fn os_string(value: OsString) -> Result<String, TshError> {
    value.into_string().map_err(|value| {
        TshError::usage(format!(
            "arguments must be valid UTF-8: {}",
            value.to_string_lossy()
        ))
    })
}

pub(crate) fn print_help() -> Result<(), TshError> {
    write_stdout(help_text())
}

pub(crate) fn help_text() -> &'static str {
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
  tools            list top-level visible tools and tool groups
  tools GROUP      list tools in a tool group, for example tools fs
  tools -l         list all visible tools with paths and descriptions
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

pub(crate) fn list_tools(root: &Path) -> Result<(), TshError> {
    list_tools_with_mode(root, ToolListMode::Groups)
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum ToolListMode {
    Groups,
    Long,
    Group(String),
}

pub(crate) fn list_tools_with_mode(root: &Path, mode: ToolListMode) -> Result<(), TshError> {
    let tool_path = ctx_tool_path(root)?;
    let hits = tool_path.list().map_err(tool_path_error)?;
    let mut entries = Vec::new();
    for hit in hits {
        let Some(name) = hit.path().file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        entries.push(ToolListEntry {
            name: name.to_owned(),
            path: hit.path().to_path_buf(),
            description: tool_description(&hit),
        });
    }
    let mut stdout = io::stdout().lock();
    match mode {
        ToolListMode::Groups => {
            for name in top_level_tool_names(&entries) {
                writeln!(stdout, "{name}").map_err(|error| write_error_to_tsh(&error))?;
            }
        }
        ToolListMode::Long => {
            for entry in &entries {
                write_tool_list_entry(&mut stdout, entry)?;
            }
        }
        ToolListMode::Group(group) => {
            let mut matched = false;
            for entry in entries
                .iter()
                .filter(|entry| tool_is_in_group(&entry.name, &group))
            {
                matched = true;
                writeln!(stdout, "{}", entry.name).map_err(|error| write_error_to_tsh(&error))?;
            }
            if !matched {
                writeln!(stdout, "tsh: tool group not found: {group}\ntry: tools")
                    .map_err(|error| write_error_to_tsh(&error))?;
            }
        }
    }
    stdout.flush().map_err(|error| write_error_to_tsh(&error))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ToolListEntry {
    name: String,
    path: PathBuf,
    description: String,
}

pub(crate) fn write_tool_list_entry(
    stdout: &mut impl Write,
    entry: &ToolListEntry,
) -> Result<(), TshError> {
    if entry.description.is_empty() {
        writeln!(stdout, "{}\t{}", entry.name, entry.path.display())
            .map_err(|error| write_error_to_tsh(&error))
    } else {
        writeln!(
            stdout,
            "{}\t{}\t{}",
            entry.name,
            entry.path.display(),
            entry.description
        )
        .map_err(|error| write_error_to_tsh(&error))
    }
}

pub(crate) fn top_level_tool_names(entries: &[ToolListEntry]) -> Vec<String> {
    let mut names = BTreeSet::new();
    for entry in entries {
        if let Some((group, _leaf)) = entry.name.split_once('.')
            && !group.is_empty()
        {
            let _inserted = names.insert(format!("{group}."));
            continue;
        }
        let _inserted = names.insert(entry.name.clone());
    }
    names.into_iter().collect()
}

pub(crate) fn tool_is_in_group(name: &str, group: &str) -> bool {
    let group = group.trim_end_matches('.');
    !group.is_empty()
        && name
            .strip_prefix(group)
            .is_some_and(|suffix| suffix.starts_with('.') && suffix.len() > 1)
}

pub(crate) fn read_tsh_config(
    root: &Path,
) -> Result<cortexfs::tool::core::tools::TshRuntimeConfig, TshError> {
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

pub(crate) fn read_tsh_config_text(root: &Path) -> Result<Option<String>, TshError> {
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

#[path = "tsh/context.rs"]
pub mod context;

#[path = "tsh/terminal.rs"]
pub mod terminal;

#[path = "shared/no-follow-fs.rs"]
pub mod no_follow_fs;
#[path = "shared/plain-dir.rs"]
pub mod plain_dir;
#[path = "shared/proc-fd.rs"]
pub mod proc_fd;
#[path = "tsh/repl.rs"]
pub mod repl;
#[path = "tsh/repl-parse.rs"]
pub mod repl_parse;
#[path = "shared/small-text.rs"]
pub mod small_text;

pub(crate) fn load_persistent_context(
    root: &Path,
    max_loaded_tools: usize,
) -> Result<ToolContext, TshError> {
    let Some(path) = persistent_context_path(root)? else {
        return Ok(ToolContext::new(max_loaded_tools));
    };
    let state = cortexfs::read_tsh_context_state(&path).map_err(|error| {
        TshError::unavailable(format!("cannot read {}: {error}", path.display()))
    })?;
    Ok(ToolContext::from_state(state, max_loaded_tools))
}

pub(crate) fn persist_context(root: &Path, context: &ToolContext) -> Result<(), TshError> {
    let Some(path) = persistent_context_path(root)? else {
        return Ok(());
    };
    cortexfs::write_tsh_context_state(&path, &context.to_state())
        .map_err(|error| TshError::unavailable(format!("cannot write {}: {error}", path.display())))
}

pub(crate) fn persistent_context_path(root: &Path) -> Result<Option<PathBuf>, TshError> {
    let Some(agent) = env::var("CTX_AGENT").ok() else {
        return Ok(None);
    };
    let view =
        derive_agent_runtime_view(root, &agent).map_err(|error| agent_view_error_to_tsh(&error))?;
    Ok(Some(cortexfs::tsh_context_state_path(view.home())))
}

pub(crate) fn restore_persistent_cache(cache: &mut DynamicToolCache, context: &ToolContext) {
    for tool in context.values() {
        if tool.pinned {
            cache.pin_path(&tool.path);
        } else if tool.dynamic_resident {
            cache.load_path(&tool.path);
        }
    }
}

pub(crate) fn run_tool(root: &Path, name: &str, args: Vec<OsString>) -> Result<ExitCode, TshError> {
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

pub(crate) fn run_tool_with_context(
    root: &Path,
    cache: &mut DynamicToolCache,
    context: &mut ToolContext,
    name: &str,
    args: Vec<OsString>,
) -> Result<ExitCode, TshError> {
    authorize_tsh_tool_execution(root, name)?;
    let mut loaded = load_tool_context(root, name, false)?;
    cache.load_path(&loaded.path);
    loaded.dynamic_resident = cache.contains_path(&loaded.path);
    let evicted = context.insert(loaded);
    persist_context(root, context)?;
    report_context_evictions(evicted)?;
    run_tool(root, name, args)
}

pub(crate) fn authorize_tsh_tool_execution(
    root: &Path,
    name: &str,
) -> Result<AuthorizedTshTool, TshError> {
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

pub(crate) fn agent_name_from_env() -> Result<String, TshError> {
    env::var("CTX_AGENT").map_err(|error| match error {
        env::VarError::NotPresent => TshError::unavailable(
            "cannot authorize tool execution: CTX_AGENT is not set; use `ctx agent attach AGENT` to run tools in an agent terminal",
        ),
        env::VarError::NotUnicode(_value) => TshError::usage("CTX_AGENT must be UTF-8"),
    })
}

pub(crate) fn agent_view_error_to_tsh(error: &AgentRuntimeViewError) -> TshError {
    TshError::unavailable(format!("cannot derive agent authority: {}", error.errno()))
}

pub(crate) fn tool_execution_denial_to_tsh(name: &str, denial: ToolExecutionDenial) -> TshError {
    TshError::unavailable(format!("cannot execute tool:{name}: {}", denial.errno()))
}

#[path = "tsh/tool-lookup.rs"]
pub mod tool_lookup;

pub(crate) fn write_stdout(message: &str) -> Result<(), TshError> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(message.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|error| write_error_to_tsh(&error))
}

pub(crate) fn report_repl_error(error: &TshError) -> Result<(), TshError> {
    write_error(&format!("tsh: {}", error.message)).map_err(|error| write_error_to_tsh(&error))
}

pub(crate) fn write_error_to_tsh(error: &io::Error) -> TshError {
    TshError::unavailable(format!("cannot write output: {error}"))
}

#[cfg(test)]
#[expect(
    unused_qualifications,
    unused_imports,
    reason = "tests use qualified paths / imports inherited by submodules"
)]
#[path = "tsh/tests"]
mod tests {
    use super::{
        DynamicToolCache, LoadedTool, MAX_TSH_REPL_LINE_BYTES, ToolContext, ToolListEntry,
        TshCommand, append_schema_help, builtin_words, ctx_tool_path_with_home, help_text,
        load_tool_context, open_executable_no_follow, parse_args, parse_repl_line,
        parse_tshrc_ctx_path, read_repl_line_canonical_from, read_tsh_config_text,
        requires_explicit_repl_input, run_repl_tool, run_tool, terminal_safe_text, tiny_lfu_admits,
        tool_diagnostic_help_text, tool_is_in_group, top_level_tool_names, tshrc_ctx_path,
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

    #[path = "argument-path.rs"]
    pub(crate) mod argument_path;
    #[path = "context-cache.rs"]
    pub(crate) mod context_cache;
    #[path = "execution.rs"]
    pub(crate) mod execution;
}
