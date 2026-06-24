use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as FmtWrite;
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode, Stdio};
use std::{env, fs};

use cortexfs::{CTX_ROOT, ToolPath};
use cortexfs_tool_sdk::{DynamicToolCache, ToolInvocation, run_tool as run_sdk_tool};
use serde_json::Value;

#[derive(Debug, Eq, PartialEq)]
struct TshError {
    code: u8,
    message: String,
}

impl TshError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self {
            code: 69,
            message: message.into(),
        }
    }
}

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
    let mut cache = DynamicToolCache::new(tool_cache_capacity());
    let mut context = ToolContext::default();
    match command {
        TshCommand::Help => print_help().map(|()| ExitCode::SUCCESS),
        TshCommand::List => list_tools(&root).map(|()| ExitCode::SUCCESS),
        TshCommand::Repl => run_repl(&root, &mut cache, &mut context),
        TshCommand::Tool { name, args } => run_tool(&root, &mut cache, &name, args),
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
    write_stdout(
        "\
tsh - CortexFS tool shell

usage:
  tsh [--root PATH] [--list]
  tsh [--root PATH] TOOL [ARG...]

principles:
  tsh resolves TOOL through CTX_PATH
  when CTX_PATH is unset, tsh may read CTX_HOME/.tshrc
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
  pin TOOL         load TOOL and keep it from context/cache eviction
  unpin TOOL       allow a pinned tool to be unloaded/evicted again
  pins             list pinned tool context entries
  bash             enter an interactive shell tool
  fs.read PATH     read a file through the fs.read tool
  exit             leave tsh
",
    )
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct LoadedTool {
    name: String,
    path: PathBuf,
    description: String,
    schema: Option<String>,
    dynamic_resident: bool,
    pinned: bool,
}

#[derive(Debug, Default, Eq, PartialEq)]
struct ToolContext {
    tools: BTreeMap<String, LoadedTool>,
}

impl ToolContext {
    fn insert(&mut self, tool: LoadedTool) {
        let _old = self.tools.insert(tool.name.clone(), tool);
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut LoadedTool> {
        self.tools.get_mut(name)
    }

    fn remove_unpinned(&mut self, name: &str) -> Result<Option<LoadedTool>, TshError> {
        if self.tools.get(name).is_some_and(|tool| tool.pinned) {
            return Err(TshError::unavailable(format!(
                "{name} is pinned; run `unpin {name}` before unload"
            )));
        }
        Ok(self.tools.remove(name))
    }

    fn values(&self) -> impl Iterator<Item = &LoadedTool> {
        self.tools.values()
    }

    fn pinned_values(&self) -> impl Iterator<Item = &LoadedTool> {
        self.tools.values().filter(|tool| tool.pinned)
    }
}

fn run_repl(
    root: &Path,
    cache: &mut DynamicToolCache,
    context: &mut ToolContext,
) -> Result<ExitCode, TshError> {
    loop {
        write_stdout("tsh> ")?;
        let mut line = String::new();
        let bytes = {
            let stdin = io::stdin();
            let mut input = stdin.lock();
            input
                .read_line(&mut line)
                .map_err(|error| read_error_to_tsh(&error))?
        };
        if bytes == 0 {
            return Ok(ExitCode::SUCCESS);
        }
        let words = match parse_repl_line(&line) {
            Ok(words) => words,
            Err(error) => {
                report_repl_error(&error)?;
                continue;
            }
        };
        if words.is_empty() {
            continue;
        }
        match words.first().map(String::as_str) {
            Some("exit" | "quit") => match parse_exit_code(&words) {
                Ok(code) => return Ok(code),
                Err(error) => report_repl_error(&error)?,
            },
            Some("help") => {
                if let Err(error) = repl_help(root, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("tools") => {
                if let Err(error) = repl_tools(root, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("which") => {
                if let Err(error) = repl_which(root, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("type") => {
                if let Err(error) = repl_type(root, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("command") => {
                if let Err(error) = repl_command(root, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("load") => {
                if let Err(error) = repl_load(root, cache, context, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("unload") => {
                if let Err(error) = repl_unload(root, cache, context, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("loads") => {
                if let Err(error) = repl_loads(context, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("pin") => {
                if let Err(error) = repl_pin(root, cache, context, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("unpin") => {
                if let Err(error) = repl_unpin(root, cache, context, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some("pins") => {
                if let Err(error) = repl_pins(context, &words) {
                    report_repl_error(&error)?;
                }
            }
            Some(name) => {
                let args = words.iter().skip(1).map(OsString::from).collect::<Vec<_>>();
                if let Err(error) = run_repl_tool(root, cache, name, args) {
                    report_repl_error(&error)?;
                }
            }
            None => {}
        }
    }
}

fn repl_help(root: &Path, words: &[String]) -> Result<(), TshError> {
    if words.len() == 1 {
        return print_help();
    }
    if words.len() != 2 {
        return write_stdout("tsh: help accepts at most one topic\n");
    }
    let Some(name) = words.get(1) else {
        return print_help();
    };
    if is_tsh_builtin(name) {
        print_builtin_help(name)
    } else {
        print_tool_help(root, name)
    }
}

fn repl_tools(root: &Path, words: &[String]) -> Result<(), TshError> {
    if words.len() == 1 {
        return list_tools_with_mode(root, ToolListMode::Names);
    }
    if words.len() == 2
        && words
            .get(1)
            .is_some_and(|flag| flag == "-l" || flag == "--long")
    {
        return list_tools_with_mode(root, ToolListMode::Long);
    }
    write_stdout("tsh: tools accepts only -l/--long\n")
}

fn repl_which(root: &Path, words: &[String]) -> Result<(), TshError> {
    if words.len() == 2 {
        let Some(name) = words.get(1) else {
            return write_stdout("tsh: which requires a tool name\n");
        };
        print_tool_path(root, name)
    } else if words.len() == 1 {
        write_stdout("tsh: which requires a tool name\n")
    } else {
        write_stdout("tsh: which accepts one tool name\n")
    }
}

fn repl_type(root: &Path, words: &[String]) -> Result<(), TshError> {
    if words.len() == 1 {
        return write_stdout("tsh: type requires a tool name\n");
    }
    if words.len() != 2 {
        return write_stdout("tsh: type accepts one tool name\n");
    }
    let Some(name) = words.get(1) else {
        return write_stdout("tsh: type requires a tool name\n");
    };
    print_command_type(root, name)
}

fn repl_command(root: &Path, words: &[String]) -> Result<(), TshError> {
    if words.len() == 3 && words.get(1).is_some_and(|flag| flag == "-v") {
        let Some(name) = words.get(2) else {
            return write_stdout("tsh: command supports only `command -v TOOL`\n");
        };
        return print_command_v(root, name);
    }
    write_stdout("tsh: command supports only `command -v TOOL`\n")
}

fn repl_load(
    root: &Path,
    cache: &mut DynamicToolCache,
    context: &mut ToolContext,
    words: &[String],
) -> Result<(), TshError> {
    if words.len() != 2 {
        return write_stdout("tsh: load requires one tool name\n");
    }
    let Some(name) = words.get(1) else {
        return write_stdout("tsh: load requires one tool name\n");
    };
    let loaded = load_tool_context(root, cache, name, false)?;
    let loaded_name = loaded.name.clone();
    let path = loaded.path.clone();
    let dynamic_resident = loaded.dynamic_resident;
    context.insert(loaded);
    let state = if dynamic_resident {
        "metadata+resident"
    } else {
        "metadata"
    };
    write_stdout(&format!(
        "loaded {loaded_name}\t{}\t{state}\n",
        path.display()
    ))
}

fn repl_unload(
    root: &Path,
    cache: &mut DynamicToolCache,
    context: &mut ToolContext,
    words: &[String],
) -> Result<(), TshError> {
    if words.len() != 2 {
        return write_stdout("tsh: unload requires one tool name\n");
    }
    let Some(name) = words.get(1) else {
        return write_stdout("tsh: unload requires one tool name\n");
    };
    let loaded = context.remove_unpinned(name)?;
    let Some(loaded) = loaded else {
        return write_stdout(&format!("{name} is not loaded\n"));
    };
    let hit = resolve_tool_hit(root, name)?;
    let _was_pinned = cache.unpin_path(hit.path());
    write_stdout(&format!(
        "unloaded {}\t{}\n",
        loaded.name,
        loaded.path.display()
    ))
}

fn repl_loads(context: &ToolContext, words: &[String]) -> Result<(), TshError> {
    if words.len() != 1 {
        return write_stdout("tsh: loads does not accept arguments\n");
    }
    print_loaded_tools(context.values())
}

fn repl_pin(
    root: &Path,
    cache: &mut DynamicToolCache,
    context: &mut ToolContext,
    words: &[String],
) -> Result<(), TshError> {
    if words.len() != 2 {
        return write_stdout("tsh: pin requires one tool name\n");
    }
    let Some(name) = words.get(1) else {
        return write_stdout("tsh: pin requires one tool name\n");
    };
    let loaded = load_tool_context(root, cache, name, true)?;
    let loaded_name = loaded.name.clone();
    let path = loaded.path.clone();
    let dynamic_resident = loaded.dynamic_resident;
    context.insert(loaded);
    let state = if dynamic_resident {
        "pinned metadata+resident"
    } else {
        "pinned metadata"
    };
    write_stdout(&format!("{state} {loaded_name}\t{}\n", path.display()))
}

fn repl_unpin(
    root: &Path,
    cache: &mut DynamicToolCache,
    context: &mut ToolContext,
    words: &[String],
) -> Result<(), TshError> {
    if words.len() != 2 {
        return write_stdout("tsh: unpin requires one tool name\n");
    }
    let Some(name) = words.get(1) else {
        return write_stdout("tsh: unpin requires one tool name\n");
    };
    let hit = resolve_tool_hit(root, name)?;
    let memory_unpinned = cache.unpin_path(hit.path());
    if let Some(loaded) = context.get_mut(name) {
        loaded.pinned = false;
        if !memory_unpinned {
            loaded.dynamic_resident = false;
        }
        write_stdout(&format!("unpinned {name}\t{}\n", hit.path().display()))
    } else {
        write_stdout(&format!("{name} is not loaded\n"))
    }
}

fn repl_pins(context: &ToolContext, words: &[String]) -> Result<(), TshError> {
    if words.len() != 1 {
        return write_stdout("tsh: pins does not accept arguments\n");
    }
    print_loaded_tools(context.pinned_values())
}

fn print_loaded_tools<'a>(tools: impl Iterator<Item = &'a LoadedTool>) -> Result<(), TshError> {
    let mut stdout = io::stdout().lock();
    for tool in tools {
        let state = match (tool.pinned, tool.dynamic_resident) {
            (true, true) => "pinned,resident",
            (true, false) => "pinned",
            (false, true) => "resident",
            (false, false) => "metadata",
        };
        if tool.description.is_empty() {
            writeln!(stdout, "{}\t{}\t{state}", tool.name, tool.path.display())
                .map_err(|error| write_error_to_tsh(&error))?;
        } else {
            writeln!(
                stdout,
                "{}\t{}\t{state}\t{}",
                tool.name,
                tool.path.display(),
                tool.description
            )
            .map_err(|error| write_error_to_tsh(&error))?;
        }
    }
    stdout.flush().map_err(|error| write_error_to_tsh(&error))
}

fn print_tool_path(root: &Path, name: &str) -> Result<(), TshError> {
    let tool_path = ctx_tool_path(root)?;
    let Some(hit) = tool_path.find(name).map_err(tool_path_error)? else {
        return write_stdout(&format!(
            "tsh: tool not found in CTX_PATH: {name}\ntry: tools\n"
        ));
    };
    write_stdout(&format!("{}\n", hit.path().display()))
}

fn print_command_type(root: &Path, name: &str) -> Result<(), TshError> {
    if is_tsh_builtin(name) {
        return write_stdout(&format!("{name} is a tsh builtin\n"));
    }
    let tool_path = ctx_tool_path(root)?;
    let Some(hit) = tool_path.find(name).map_err(tool_path_error)? else {
        return command_not_found(name);
    };
    write_stdout(&format!("{name} is {}\n", hit.path().display()))
}

fn print_command_v(root: &Path, name: &str) -> Result<(), TshError> {
    if is_tsh_builtin(name) {
        return write_stdout(&format!("{name}\n"));
    }
    print_tool_path(root, name)
}

fn print_builtin_help(name: &str) -> Result<(), TshError> {
    let text = match name {
        "exit" | "quit" => "exit [CODE]\n  leave tsh\n",
        "help" => "help [TOOL]\n  show tsh help or visible tool metadata\n",
        "tools" => "tools [-l]\n  list visible tools from CTX_PATH\n",
        "which" => "which TOOL\n  print the resolved tool path\n",
        "type" => "type TOOL\n  show whether TOOL is a tsh builtin or visible tool\n",
        "command" => "command -v TOOL\n  print the command that tsh would run\n",
        "load" => "load TOOL\n  load tool metadata into this tsh context\n",
        "unload" => "unload TOOL\n  remove unpinned tool metadata from this tsh context\n",
        "loads" => "loads\n  list loaded tool context entries\n",
        "pin" => "pin TOOL\n  load TOOL and keep it from context/cache eviction\n",
        "unpin" => "unpin TOOL\n  allow a pinned tool to be unloaded/evicted again\n",
        "pins" => "pins\n  list pinned tool context entries\n",
        _ => "unknown builtin\n",
    };
    write_stdout(text)
}

fn print_tool_help(root: &Path, name: &str) -> Result<(), TshError> {
    let tool_path = ctx_tool_path(root)?;
    let Some(hit) = tool_path.find(name).map_err(tool_path_error)? else {
        return command_not_found(name);
    };
    let description = tool_description(&hit);
    let schema = tool_schema(&hit);
    let mut text = format!("{name}\n  path: {}\n", hit.path().display());
    if !description.is_empty() {
        let _ignored = writeln!(text, "  description: {description}");
    }
    if let Some(schema) = schema {
        append_schema_help(&mut text, &schema);
    }
    write_stdout(&text)
}

fn run_repl_tool(
    root: &Path,
    cache: &mut DynamicToolCache,
    name: &str,
    args: Vec<OsString>,
) -> Result<ExitCode, TshError> {
    let tool_path = ctx_tool_path(root)?;
    if tool_path.find(name).map_err(tool_path_error)?.is_none() {
        return command_not_found(name);
    }
    if args.len() == 1
        && matches!(
            args.first().and_then(|arg| arg.to_str()),
            Some("-h" | "--help")
        )
    {
        return print_tool_help(root, name).map(|()| ExitCode::SUCCESS);
    }
    if args.is_empty() && !is_interactive_tool(name) {
        write_stdout(&format!(
            "tsh: {name} needs input; pass arguments instead of leaving stdin open\ntry: {name} PATH or {name} '{{\"path\":\"PATH\"}}'\n"
        ))?;
        return Ok(ExitCode::from(2));
    }
    run_tool(root, cache, name, args)
}

fn is_tsh_builtin(name: &str) -> bool {
    matches!(
        name,
        "exit"
            | "quit"
            | "help"
            | "tools"
            | "which"
            | "type"
            | "command"
            | "load"
            | "unload"
            | "loads"
            | "pin"
            | "unpin"
            | "pins"
    )
}

fn is_interactive_tool(name: &str) -> bool {
    matches!(name, "bash" | "tmux" | "zellij" | "tsh")
}

fn parse_exit_code(words: &[String]) -> Result<ExitCode, TshError> {
    match *words {
        [_] => Ok(ExitCode::SUCCESS),
        [_, ref code] => {
            let code = code
                .parse::<u8>()
                .map_err(|_error| TshError::usage("exit code must be 0..255"))?;
            Ok(ExitCode::from(code))
        }
        _ => Err(TshError::usage("exit accepts at most one code")),
    }
}

fn parse_repl_line(line: &str) -> Result<Vec<String>, TshError> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escape = false;
    for character in line.trim_end_matches(['\n', '\r']).chars() {
        if escape {
            word.push(character);
            escape = false;
            continue;
        }
        match (quote, character) {
            (_, '\\') => escape = true,
            (Some(active), candidate) if candidate == active => quote = None,
            (Some(_active), candidate) => word.push(candidate),
            (None, '\'' | '"') => quote = Some(character),
            (None, candidate) if candidate.is_whitespace() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            (None, candidate) => word.push(candidate),
        }
    }
    if escape {
        return Err(TshError::usage("line ends with unfinished escape"));
    }
    if quote.is_some() {
        return Err(TshError::usage("line has unterminated quote"));
    }
    if !word.is_empty() {
        words.push(word);
    }
    Ok(words)
}

fn run_tool(
    root: &Path,
    cache: &mut DynamicToolCache,
    name: &str,
    args: Vec<OsString>,
) -> Result<ExitCode, TshError> {
    let tool_path = ctx_tool_path(root)?;
    let Some(hit) = tool_path.find(name).map_err(tool_path_error)? else {
        return command_not_found(name);
    };
    if args.len() == 1
        && matches!(
            args.first().and_then(|arg| arg.to_str()),
            Some("-h" | "--help")
        )
    {
        return print_tool_help(root, name).map(|()| ExitCode::SUCCESS);
    }
    if let Ok(tool) = cache.get_or_load(hit.path()) {
        let input = collect_tool_input(&args)?;
        let run_id = env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
        let invocation = ToolInvocation::new(run_id, input);
        let mut stdout = io::stdout().lock();
        run_sdk_tool(tool, &invocation, &mut stdout)
            .map_err(|error| TshError::unavailable(format!("cannot run dynamic tool: {error}")))?;
        return Ok(ExitCode::SUCCESS);
    }
    let status = ProcessCommand::new(hit.path())
        .args(args)
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

fn collect_tool_input(args: &[OsString]) -> Result<String, TshError> {
    let input = args
        .iter()
        .map(|value| value.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ");
    if !input.is_empty() {
        return Ok(input);
    }
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|error| TshError::unavailable(format!("cannot read tool input: {error}")))?;
    Ok(input)
}

fn tool_cache_capacity() -> usize {
    env::var("CTX_TOOL_CACHE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(32)
}

fn resolve_tool_hit(root: &Path, name: &str) -> Result<cortexfs::ToolHit, TshError> {
    let tool_path = ctx_tool_path(root)?;
    let Some(hit) = tool_path.find(name).map_err(tool_path_error)? else {
        return command_not_found(name);
    };
    Ok(hit)
}

fn load_tool_context(
    root: &Path,
    cache: &mut DynamicToolCache,
    name: &str,
    pinned: bool,
) -> Result<LoadedTool, TshError> {
    let hit = resolve_tool_hit(root, name)?;
    let dynamic_resident = if pinned {
        cache.pin_path(hit.path()).is_ok()
    } else {
        cache.get_or_load(hit.path()).is_ok()
    };
    Ok(LoadedTool {
        name: name.to_owned(),
        path: hit.path().to_path_buf(),
        description: tool_description(&hit),
        schema: tool_schema(&hit),
        dynamic_resident,
        pinned,
    })
}

fn tool_description(hit: &cortexfs::ToolHit) -> String {
    read_control_text(hit, "description").unwrap_or_default()
}

fn tool_schema(hit: &cortexfs::ToolHit) -> Option<String> {
    read_control_text(hit, "schema")
}

fn read_control_text(hit: &cortexfs::ToolHit, file: &str) -> Option<String> {
    fs::read_to_string(hit.control_dir().join(file))
        .ok()
        .map(|content| content.trim().to_owned())
        .filter(|content| !content.is_empty())
}

fn append_schema_help(text: &mut String, schema: &str) {
    let Ok(value) = serde_json::from_str::<Value>(schema) else {
        return;
    };
    if let Some(title) = value.get("title").and_then(Value::as_str) {
        let _ignored = writeln!(text, "  schema: {title}");
    }
    if let Some(description) = value.get("description").and_then(Value::as_str) {
        let _ignored = writeln!(text, "  schema-description: {description}");
    }
    if let Some(required) = value.get("required").and_then(Value::as_array) {
        let fields = required
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        if !fields.is_empty() {
            let _ignored = writeln!(text, "  required: {fields}");
        }
    }
}

fn command_not_found<T>(name: &str) -> Result<T, TshError> {
    Err(TshError::unavailable(format!(
        "{name}: command not found\ntry: tools"
    )))
}

fn ctx_tool_path(root: &Path) -> Result<ToolPath, TshError> {
    match env::var("CTX_PATH") {
        Ok(value) => Ok(ToolPath::parse(&value)),
        Err(env::VarError::NotPresent) => {
            let home = ctx_home(root)?;
            tshrc_ctx_path(&home)?.map_or_else(
                || Ok(ToolPath::default(root, &home)),
                |value| Ok(ToolPath::parse(&value)),
            )
        }
        Err(env::VarError::NotUnicode(_value)) => Err(TshError::usage("CTX_PATH must be UTF-8")),
    }
}

fn tshrc_ctx_path(home: &Path) -> Result<Option<String>, TshError> {
    let path = home.join(".tshrc");
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(TshError::unavailable(format!(
                "cannot read {}: {error}",
                path.display()
            )));
        }
    };
    parse_tshrc_ctx_path(&content)
        .map_err(|message| TshError::usage(format!("invalid {}: {message}", path.display())))
}

fn parse_tshrc_ctx_path(content: &str) -> Result<Option<String>, String> {
    let mut value = None;
    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(path) = line.strip_prefix("CTX_PATH=") else {
            return Err(format!(
                "line {} must be CTX_PATH=...",
                index.saturating_add(1)
            ));
        };
        if path.is_empty() {
            return Err(format!(
                "line {} has empty CTX_PATH",
                index.saturating_add(1)
            ));
        }
        if value.replace(path.to_owned()).is_some() {
            return Err(format!("line {} repeats CTX_PATH", index.saturating_add(1)));
        }
    }
    Ok(value)
}

fn ctx_home(root: &Path) -> Result<PathBuf, TshError> {
    if let Some(home) = env::var_os("CTX_HOME") {
        return Ok(PathBuf::from(home));
    }
    Ok(root.join("home").join(current_uid()?))
}

fn current_uid() -> Result<String, TshError> {
    let output = ProcessCommand::new("id")
        .arg("-u")
        .output()
        .map_err(|error| TshError::unavailable(format!("cannot run id -u: {error}")))?;
    if !output.status.success() {
        return Err(TshError::unavailable("id -u failed"));
    }
    let uid = String::from_utf8(output.stdout)
        .map_err(|_error| TshError::unavailable("id -u returned non-UTF-8 output"))?;
    let uid = uid.trim();
    if uid.is_empty() {
        return Err(TshError::unavailable("id -u returned empty output"));
    }
    Ok(uid.to_owned())
}

fn tool_path_error(error: cortexfs::ToolPathError) -> TshError {
    match error {
        cortexfs::ToolPathError::InvalidName => TshError::usage("invalid tool name"),
        cortexfs::ToolPathError::CannotReadDirectory => {
            TshError::unavailable("cannot read CTX_PATH directory")
        }
    }
}

fn write_stdout(message: &str) -> Result<(), TshError> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(message.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|error| write_error_to_tsh(&error))
}

fn write_error(message: &str) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "{message}")
}

fn report_repl_error(error: &TshError) -> Result<(), TshError> {
    write_error(&format!("tsh: {}", error.message)).map_err(|error| write_error_to_tsh(&error))
}

fn read_error_to_tsh(error: &io::Error) -> TshError {
    TshError::unavailable(format!("cannot read input: {error}"))
}

fn write_error_to_tsh(error: &io::Error) -> TshError {
    TshError::unavailable(format!("cannot write output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        TshCommand, is_interactive_tool, parse_args, parse_repl_line, parse_tshrc_ctx_path,
    };
    use std::ffi::OsString;
    use std::path::PathBuf;

    #[test]
    fn parses_tool_command_and_root() {
        let parsed = parse_args(vec![
            OsString::from("--root"),
            OsString::from("/tmp/ctx"),
            OsString::from("bash"),
            OsString::from("-lc"),
            OsString::from("pwd"),
        ]);
        assert_eq!(
            parsed,
            Ok((
                PathBuf::from("/tmp/ctx"),
                TshCommand::Tool {
                    name: "bash".to_owned(),
                    args: vec![OsString::from("-lc"), OsString::from("pwd")]
                }
            ))
        );
    }

    #[test]
    fn parses_repl_words_without_shell_operators() {
        assert_eq!(
            parse_repl_line(r#"fs.read '{"path":"/tmp/a b"}'"#),
            Ok(vec![
                "fs.read".to_owned(),
                r#"{"path":"/tmp/a b"}"#.to_owned()
            ])
        );
        assert!(parse_repl_line("bash 'unterminated").is_err());
    }

    #[test]
    fn parses_tshrc_ctx_path_as_data() {
        assert_eq!(
            parse_tshrc_ctx_path(
                "\
# user tools first for this account
CTX_PATH=/ctx/home/1000/tool:/ctx/tool
"
            ),
            Ok(Some("/ctx/home/1000/tool:/ctx/tool".to_owned()))
        );
        assert_eq!(parse_tshrc_ctx_path("# empty\n\n"), Ok(None));
        assert!(parse_tshrc_ctx_path("export CTX_PATH=/ctx/tool\n").is_err());
        assert!(parse_tshrc_ctx_path("CTX_PATH=\n").is_err());
    }

    #[test]
    fn classifies_repl_interactive_tools() {
        assert!(is_interactive_tool("bash"));
        assert!(is_interactive_tool("tmux"));
        assert!(is_interactive_tool("zellij"));
        assert!(!is_interactive_tool("fs.read"));
    }
}
