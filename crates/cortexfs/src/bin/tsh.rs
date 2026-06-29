#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt::Write as FmtWrite;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode, Stdio};
use std::{env, fs};

use cortexfs::{
    AgentRuntimeViewError, CTX_ROOT, PolicyV0, ToolExecutionAuthority, ToolExecutionDenial,
    ToolPath, authorize_tool_execution, derive_agent_runtime_view,
};
use cortexfs_tool_sdk::DynamicToolCache;
use nix::libc;
use nix::sys::termios::{self, ControlFlags, InputFlags, LocalFlags, OutputFlags, SetArg};
use serde_json::Value;

#[derive(Debug, Eq, PartialEq)]
struct TshError {
    code: u8,
    message: String,
}

const MAX_TSH_CONTROL_BYTES: u64 = 64 * 1024;
const MAX_TSH_REPL_LINE_BYTES: usize = 1024 * 1024;
const MAX_TSH_TOOL_COUNT: usize = 1024;
type TshToolEnv = Vec<(String, String)>;
type AuthorizedTshTool = (cortexfs::ToolExecutionGrant, TshToolEnv);

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
    match command {
        TshCommand::Help => return print_help().map(|()| ExitCode::SUCCESS),
        TshCommand::List => return list_tools(&root).map(|()| ExitCode::SUCCESS),
        TshCommand::Repl | TshCommand::Tool { .. } => {}
    }
    let config = TshConfig::read(&root)?;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TshConfig {
    max_loaded_tools: usize,
    cache_capacity: usize,
    window_percent: usize,
}

impl Default for TshConfig {
    fn default() -> Self {
        Self {
            max_loaded_tools: 64,
            cache_capacity: 32,
            window_percent: 1,
        }
    }
}

impl TshConfig {
    fn read(root: &Path) -> Result<Self, TshError> {
        let mut config = if let Some(content) = read_tsh_config_text(root)? {
            parse_tsh_config(&content)
                .map_err(|message| TshError::usage(format!("invalid tsh.d/config: {message}")))?
        } else {
            Self::default()
        };
        if let Some(capacity) = env::var("CTX_TOOL_CACHE")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
        {
            config.cache_capacity = capacity.clamp(1, MAX_TSH_TOOL_COUNT);
        }
        Ok(config)
    }
}

fn read_tsh_config_text(root: &Path) -> Result<Option<String>, TshError> {
    let tool_path = ctx_tool_path(root)?;
    let Some(hit) = tool_path.find("tsh").map_err(tool_path_error)? else {
        return Ok(None);
    };
    let path = hit.control_dir().join("config");
    match read_small_plain_text_file(&path) {
        Ok(content) => Ok(Some(content)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(TshError::unavailable(format!(
            "cannot read {}: {error}",
            path.display()
        ))),
    }
}

fn parse_tsh_config(content: &str) -> Result<TshConfig, String> {
    let mut config = TshConfig::default();
    for (index, line) in content.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!(
                "line {} must be key=value",
                index.saturating_add(1)
            ));
        };
        let value = value.parse::<usize>().map_err(|_error| {
            format!(
                "line {} value must be a positive integer",
                index.saturating_add(1)
            )
        })?;
        match key {
            "max_loaded_tools" | "cache_capacity" if (1..=MAX_TSH_TOOL_COUNT).contains(&value) => {
                if key == "max_loaded_tools" {
                    config.max_loaded_tools = value;
                } else {
                    config.cache_capacity = value;
                }
            }
            "window_percent" if (1..=100).contains(&value) => config.window_percent = value,
            "max_loaded_tools" | "cache_capacity" => {
                return Err(format!(
                    "line {} value must be 1..{MAX_TSH_TOOL_COUNT}",
                    index.saturating_add(1),
                ));
            }
            "window_percent" => {
                return Err(format!(
                    "line {} window_percent must be 1..100",
                    index.saturating_add(1)
                ));
            }
            _ => {
                return Err(format!(
                    "line {} has unknown key {key}",
                    index.saturating_add(1)
                ));
            }
        }
    }
    Ok(config)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct LoadedTool {
    name: String,
    path: PathBuf,
    description: String,
    schema: Option<String>,
    dynamic_resident: bool,
    pinned: bool,
    last_used: u64,
}

#[derive(Debug, Eq, PartialEq)]
struct ToolContext {
    tools: BTreeMap<String, LoadedTool>,
    max_loaded_tools: usize,
    clock: u64,
}

impl ToolContext {
    fn new(max_loaded_tools: usize) -> Self {
        Self {
            tools: BTreeMap::new(),
            max_loaded_tools: max_loaded_tools.max(1),
            clock: 0,
        }
    }

    fn insert(&mut self, mut tool: LoadedTool) -> Vec<LoadedTool> {
        self.clock = self.clock.saturating_add(1);
        tool.last_used = self.clock;
        if let Some(existing) = self.tools.get(&tool.name) {
            tool.pinned |= existing.pinned;
            if existing.path == tool.path {
                tool.dynamic_resident |= existing.dynamic_resident;
            }
        }
        let _old = self.tools.insert(tool.name.clone(), tool);
        self.evict_over_limit()
    }

    fn get_mut(&mut self, name: &str) -> Option<&mut LoadedTool> {
        self.tools.get_mut(name)
    }

    fn touch(&mut self, name: &str) {
        if let Some(tool) = self.tools.get_mut(name) {
            self.clock = self.clock.saturating_add(1);
            tool.last_used = self.clock;
        }
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

    fn evict_over_limit(&mut self) -> Vec<LoadedTool> {
        let mut evicted = Vec::new();
        while self.tools.values().filter(|tool| !tool.pinned).count() > self.max_loaded_tools {
            let Some(name) = self
                .tools
                .values()
                .filter(|tool| !tool.pinned)
                .min_by_key(|tool| (tool.last_used, tool.name.clone()))
                .map(|tool| tool.name.clone())
            else {
                break;
            };
            if let Some(tool) = self.tools.remove(&name) {
                evicted.push(tool);
            }
        }
        evicted
    }
}

struct RawTerminal<'fd> {
    fd: BorrowedFd<'fd>,
    original: termios::Termios,
}

impl<'fd> RawTerminal<'fd> {
    fn enable(stdin: &'fd io::Stdin) -> Result<Self, TshError> {
        let fd = stdin.as_fd();
        let original = termios::tcgetattr(fd).map_err(|error| {
            TshError::unavailable(format!("cannot read terminal mode: {error}"))
        })?;
        let mut raw = original.clone();
        raw.input_flags.remove(
            InputFlags::BRKINT
                | InputFlags::ICRNL
                | InputFlags::INPCK
                | InputFlags::ISTRIP
                | InputFlags::IXON,
        );
        raw.output_flags.remove(OutputFlags::OPOST);
        raw.control_flags.insert(ControlFlags::CS8);
        raw.local_flags
            .remove(LocalFlags::ECHO | LocalFlags::ICANON | LocalFlags::IEXTEN | LocalFlags::ISIG);
        if let Some(value) = raw.control_chars.get_mut(libc::VMIN) {
            *value = 1;
        }
        if let Some(value) = raw.control_chars.get_mut(libc::VTIME) {
            *value = 0;
        }
        termios::tcsetattr(fd, SetArg::TCSAFLUSH, &raw).map_err(|error| {
            TshError::unavailable(format!("cannot switch terminal to raw mode: {error}"))
        })?;
        Ok(Self { fd, original })
    }
}

impl Drop for RawTerminal<'_> {
    fn drop(&mut self) {
        let _restored = termios::tcsetattr(self.fd, SetArg::TCSAFLUSH, &self.original);
    }
}

#[derive(Clone, Copy)]
enum ReplKey {
    Byte(u8),
    Up,
    Down,
    Right,
    Left,
    Home,
    End,
}

fn read_repl_line(prompt: &str, history: &[String]) -> Result<Option<String>, TshError> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    if !stdin.is_terminal() || !stdout.is_terminal() {
        return read_repl_line_canonical(prompt);
    }

    let _raw = RawTerminal::enable(&stdin)?;
    write_stdout(prompt)?;

    let mut input = stdin.lock();
    let mut buffer = Vec::new();
    let mut cursor = 0usize;
    let mut history_cursor: Option<usize> = None;

    loop {
        match read_repl_key(&mut input)? {
            ReplKey::Byte(b'\r' | b'\n') => {
                write_stdout("\r\n")?;
                return Ok(Some(buffer.into_iter().collect()));
            }
            ReplKey::Byte(4) if buffer.is_empty() => {
                write_stdout("\r\n")?;
                return Ok(None);
            }
            ReplKey::Byte(3) => {
                write_stdout("^C\r\n")?;
                return Ok(Some(String::new()));
            }
            ReplKey::Byte(8 | 127) => {
                if cursor > 0 {
                    cursor -= 1;
                    buffer.remove(cursor);
                    redraw_repl_line(prompt, &buffer, cursor)?;
                }
            }
            ReplKey::Byte(byte) if byte.is_ascii_graphic() || byte == b' ' => {
                if buffer.len() >= MAX_TSH_REPL_LINE_BYTES {
                    return Err(TshError::usage("tsh input line exceeds limit"));
                }
                buffer.insert(cursor, char::from(byte));
                cursor += 1;
                history_cursor = None;
                redraw_repl_line(prompt, &buffer, cursor)?;
            }
            ReplKey::Up => {
                if history.is_empty() {
                    continue;
                }
                let next =
                    history_cursor.map_or(history.len() - 1, |index| index.saturating_sub(1));
                history_cursor = Some(next);
                if let Some(entry) = history.get(next) {
                    buffer = entry.chars().collect();
                    cursor = buffer.len();
                    redraw_repl_line(prompt, &buffer, cursor)?;
                }
            }
            ReplKey::Down => {
                let Some(index) = history_cursor else {
                    continue;
                };
                if index + 1 < history.len() {
                    let next = index + 1;
                    history_cursor = Some(next);
                    if let Some(entry) = history.get(next) {
                        buffer = entry.chars().collect();
                        cursor = buffer.len();
                    }
                } else {
                    history_cursor = None;
                    buffer.clear();
                    cursor = 0;
                }
                redraw_repl_line(prompt, &buffer, cursor)?;
            }
            ReplKey::Left => {
                if cursor > 0 {
                    cursor -= 1;
                    redraw_repl_line(prompt, &buffer, cursor)?;
                }
            }
            ReplKey::Right => {
                if cursor < buffer.len() {
                    cursor += 1;
                    redraw_repl_line(prompt, &buffer, cursor)?;
                }
            }
            ReplKey::Home => {
                cursor = 0;
                redraw_repl_line(prompt, &buffer, cursor)?;
            }
            ReplKey::End => {
                cursor = buffer.len();
                redraw_repl_line(prompt, &buffer, cursor)?;
            }
            ReplKey::Byte(_) => {}
        }
    }
}

fn read_repl_line_canonical(prompt: &str) -> Result<Option<String>, TshError> {
    write_stdout(prompt)?;
    let stdin = io::stdin();
    let mut stdin = stdin.lock();
    read_repl_line_canonical_from(&mut stdin)
}

fn read_repl_line_canonical_from(reader: &mut impl BufRead) -> Result<Option<String>, TshError> {
    let mut line = String::new();
    let limit = u64::try_from(MAX_TSH_REPL_LINE_BYTES.saturating_add(2))
        .map_err(|error| TshError::unavailable(format!("input limit is invalid: {error}")))?;
    let bytes = reader
        .take(limit)
        .read_line(&mut line)
        .map_err(|error| TshError::unavailable(format!("cannot read input: {error}")))?;
    if bytes == 0 {
        return Ok(None);
    }
    while line.ends_with(['\n', '\r']) {
        line.pop();
    }
    if line.len() > MAX_TSH_REPL_LINE_BYTES {
        return Err(TshError::usage("tsh input line exceeds limit"));
    }
    Ok(Some(line))
}

fn read_repl_key(input: &mut impl Read) -> Result<ReplKey, TshError> {
    let byte = read_byte(input)?;
    if byte != b'\x1b' {
        return Ok(ReplKey::Byte(byte));
    }

    let introducer = read_byte(input)?;
    if introducer != b'[' {
        return Ok(ReplKey::Byte(byte));
    }

    match read_byte(input)? {
        b'A' => Ok(ReplKey::Up),
        b'B' => Ok(ReplKey::Down),
        b'C' => Ok(ReplKey::Right),
        b'D' => Ok(ReplKey::Left),
        b'H' => Ok(ReplKey::Home),
        b'F' => Ok(ReplKey::End),
        b'1' | b'7' => {
            let _tilde = read_byte(input)?;
            Ok(ReplKey::Home)
        }
        b'4' | b'8' => {
            let _tilde = read_byte(input)?;
            Ok(ReplKey::End)
        }
        _ => Ok(ReplKey::Byte(byte)),
    }
}

fn read_byte(input: &mut impl Read) -> Result<u8, TshError> {
    let mut byte = [0u8; 1];
    input
        .read_exact(&mut byte)
        .map_err(|error| TshError::unavailable(format!("cannot read terminal input: {error}")))?;
    Ok(byte[0])
}

fn redraw_repl_line(prompt: &str, buffer: &[char], cursor: usize) -> Result<(), TshError> {
    let text: String = buffer.iter().collect();
    write_stdout(&format!("\r{prompt}{text}\x1b[K"))?;
    let right = buffer.len().saturating_sub(cursor);
    if right > 0 {
        write_stdout(&format!("\x1b[{right}D"))?;
    }
    Ok(())
}

fn push_history(history: &mut Vec<String>, line: &str) {
    if history.last().is_some_and(|entry| entry == line) {
        return;
    }
    history.push(line.to_owned());
}

fn run_repl(
    root: &Path,
    cache: &mut DynamicToolCache,
    context: &mut ToolContext,
) -> Result<ExitCode, TshError> {
    let mut history = Vec::new();
    loop {
        let Some(line) = read_repl_line("tsh> ", &history)? else {
            return Ok(ExitCode::SUCCESS);
        };
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
        push_history(&mut history, line.as_str());
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
                if let Err(error) = run_repl_tool(root, context, name, args) {
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

fn run_builtin_once(
    root: &Path,
    cache: &mut DynamicToolCache,
    context: &mut ToolContext,
    name: &str,
    args: Vec<OsString>,
) -> Result<ExitCode, TshError> {
    let words = builtin_words(name, args)?;
    match name {
        "exit" | "quit" => parse_exit_code(&words),
        "help" => repl_help(root, &words).map(|()| ExitCode::SUCCESS),
        "tools" => repl_tools(root, &words).map(|()| ExitCode::SUCCESS),
        "which" => repl_which(root, &words).map(|()| ExitCode::SUCCESS),
        "type" => repl_type(root, &words).map(|()| ExitCode::SUCCESS),
        "command" => repl_command(root, &words).map(|()| ExitCode::SUCCESS),
        "load" => repl_load(root, cache, context, &words).map(|()| ExitCode::SUCCESS),
        "unload" => repl_unload(root, cache, context, &words).map(|()| ExitCode::SUCCESS),
        "loads" => repl_loads(context, &words).map(|()| ExitCode::SUCCESS),
        "pin" => repl_pin(root, cache, context, &words).map(|()| ExitCode::SUCCESS),
        "unpin" => repl_unpin(root, cache, context, &words).map(|()| ExitCode::SUCCESS),
        "pins" => repl_pins(context, &words).map(|()| ExitCode::SUCCESS),
        _ => command_not_found(name),
    }
}

fn builtin_words(name: &str, args: Vec<OsString>) -> Result<Vec<String>, TshError> {
    let mut words = Vec::with_capacity(args.len() + 1);
    words.push(name.to_owned());
    for arg in args {
        words.push(os_string(arg)?);
    }
    Ok(words)
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
    cache: &DynamicToolCache,
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
    let evicted = context.insert(loaded);
    let state = if dynamic_resident {
        "metadata+resident"
    } else {
        "metadata"
    };
    write_stdout(&format!(
        "loaded {loaded_name}\t{}\t{state}\n",
        path.display()
    ))?;
    report_context_evictions(evicted)
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
    cache: &DynamicToolCache,
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
    let evicted = context.insert(loaded);
    let state = if dynamic_resident {
        "pinned metadata+resident"
    } else {
        "pinned metadata"
    };
    write_stdout(&format!("{state} {loaded_name}\t{}\n", path.display()))?;
    report_context_evictions(evicted)
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
        "pin" => "pin TOOL\n  load TOOL metadata and keep it from context eviction\n",
        "unpin" => "unpin TOOL\n  allow a pinned tool to be unloaded from context again\n",
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
    context: &mut ToolContext,
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
    if args.is_empty() && requires_explicit_repl_input(name) {
        write_stdout(&format!(
            "tsh: {name} needs input; pass arguments instead of leaving stdin open\ntry: {name} PATH or {name} '{{\"path\":\"PATH\"}}'\n"
        ))?;
        return Ok(ExitCode::from(2));
    }
    context.touch(name);
    run_tool(root, name, args)
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

fn requires_explicit_repl_input(name: &str) -> bool {
    matches!(name, "fs.read" | "fs.write" | "shell.exec")
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
    let policy_text = read_small_plain_text_file(&policy_path).map_err(|error| {
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

fn resolve_tool_hit(root: &Path, name: &str) -> Result<cortexfs::ToolHit, TshError> {
    let tool_path = ctx_tool_path(root)?;
    let Some(hit) = tool_path.find(name).map_err(tool_path_error)? else {
        return command_not_found(name);
    };
    Ok(hit)
}

fn load_tool_context(
    root: &Path,
    cache: &DynamicToolCache,
    name: &str,
    pinned: bool,
) -> Result<LoadedTool, TshError> {
    let hit = resolve_tool_hit(root, name)?;
    let dynamic_resident = cache.contains_path(&hit.path().display().to_string());
    Ok(LoadedTool {
        name: name.to_owned(),
        path: hit.path().to_path_buf(),
        description: tool_description(&hit),
        schema: tool_schema(&hit),
        dynamic_resident,
        pinned,
        last_used: 0,
    })
}

fn report_context_evictions(evicted: Vec<LoadedTool>) -> Result<(), TshError> {
    for tool in evicted {
        write_stdout(&format!("auto-unloaded {}\tcontext-limit\n", tool.name))?;
    }
    Ok(())
}

fn tool_description(hit: &cortexfs::ToolHit) -> String {
    read_control_text(hit, "description")
        .map(|description| terminal_safe_text(&description))
        .unwrap_or_default()
}

fn tool_schema(hit: &cortexfs::ToolHit) -> Option<String> {
    read_control_text(hit, "schema")
}

fn read_control_text(hit: &cortexfs::ToolHit, file: &str) -> Option<String> {
    read_small_plain_text_file(&hit.control_dir().join(file))
        .ok()
        .map(|content| content.trim().to_owned())
        .filter(|content| !content.is_empty())
}

fn read_small_plain_text_file(path: &Path) -> io::Result<String> {
    let mut file = open_plain_read_file(path)?;
    let metadata = file.metadata()?;
    if metadata.len() > MAX_TSH_CONTROL_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds tsh control read limit",
        ));
    }
    let len = usize::try_from(metadata.len()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file is too large to read: {error}"),
        )
    })?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.utf8_error()))
}

fn open_plain_read_file(path: &Path) -> io::Result<fs::File> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    let parent_dir = open_plain_directory(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )?;
    let file = fs::File::from(file_fd);
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "path is not a regular file",
        ));
    }
    Ok(file)
}

fn open_executable_no_follow(path: &Path) -> io::Result<fs::File> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    let parent_dir = open_plain_directory(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW,
        nix::sys::stat::Mode::empty(),
    )?;
    let file = fs::File::from(file_fd);
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "path is not a regular file",
        ));
    }
    Ok(file)
}

fn proc_fd_path(file: &fs::File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

fn open_plain_directory(path: &Path) -> io::Result<fs::File> {
    let mut directory = if path.is_absolute() {
        open_single_plain_directory(Path::new("/"))?
    } else {
        open_single_plain_directory(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid directory name")
                })?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_single_plain_directory(path: &Path) -> io::Result<fs::File> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ));
    }
    Ok(directory)
}

fn terminal_safe_text(text: &str) -> String {
    text.chars().flat_map(char::escape_default).collect()
}

fn append_schema_help(text: &mut String, schema: &str) {
    let Ok(value) = serde_json::from_str::<Value>(schema) else {
        return;
    };
    if let Some(title) = value.get("title").and_then(Value::as_str) {
        let title = terminal_safe_text(title);
        let _ignored = writeln!(text, "  schema: {title}");
    }
    if let Some(description) = value.get("description").and_then(Value::as_str) {
        let description = terminal_safe_text(description);
        let _ignored = writeln!(text, "  schema-description: {description}");
    }
    if let Some(required) = value.get("required").and_then(Value::as_array) {
        let fields = required
            .iter()
            .filter_map(Value::as_str)
            .map(terminal_safe_text)
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
    let home = ctx_home(root)?;
    ctx_tool_path_with_home(
        root,
        &home,
        env::var("CTX_PATH"),
        env::var_os("CTX_AGENT").is_none(),
    )
}

fn ctx_tool_path_with_home(
    root: &Path,
    home: &Path,
    env_ctx_path: Result<String, env::VarError>,
    prefer_tshrc: bool,
) -> Result<ToolPath, TshError> {
    if prefer_tshrc && let Some(value) = tshrc_ctx_path(root, home)? {
        return Ok(tshrc_tool_path(root, home, &value));
    }

    match env_ctx_path {
        Ok(value) => Ok(ToolPath::parse(&value)),
        Err(env::VarError::NotPresent) => tshrc_ctx_path(root, home)?.map_or_else(
            || Ok(ToolPath::default(root, home)),
            |value| Ok(tshrc_tool_path(root, home, &value)),
        ),
        Err(env::VarError::NotUnicode(_value)) => Err(TshError::usage("CTX_PATH must be UTF-8")),
    }
}

fn tshrc_tool_path(root: &Path, home: &Path, value: &str) -> ToolPath {
    ToolPath::new(value.split(':').map(|component| {
        let path = Path::new(component);
        if path == Path::new("/ctx/tool") {
            return root.join("tool");
        }
        if let Some(uid) = home.file_name()
            && path == Path::new("/ctx/home").join(uid).join("tool")
        {
            return home.join("tool");
        }
        path.to_path_buf()
    }))
}

fn tshrc_ctx_path(root: &Path, home: &Path) -> Result<Option<String>, TshError> {
    let path = home.join(".tshrc");
    let content = match read_small_plain_text_file(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(TshError::unavailable(format!(
                "cannot read {}: {error}",
                path.display()
            )));
        }
    };
    let value = parse_tshrc_ctx_path(&content)
        .map_err(|message| TshError::usage(format!("invalid {}: {message}", path.display())))?;
    if let Some(ref value) = value {
        validate_tshrc_ctx_path(value, root, home)
            .map_err(|message| TshError::usage(format!("invalid {}: {message}", path.display())))?;
    }
    Ok(value)
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

fn validate_tshrc_ctx_path(value: &str, root: &Path, home: &Path) -> Result<(), String> {
    for component in value.split(':') {
        if component.is_empty() {
            return Err("CTX_PATH contains an empty component".to_owned());
        }
        let path = Path::new(component);
        if !path.is_absolute() {
            return Err(format!("CTX_PATH component is not absolute: {component}"));
        }
        if is_allowed_tshrc_tool_dir(path, root, home) {
            continue;
        }
        return Err(format!(
            "CTX_PATH component must be /ctx/tool, /ctx/home/<uid>/tool, or the matching --root/CTX_HOME tool directory: {component}"
        ));
    }
    Ok(())
}

fn is_allowed_tshrc_tool_dir(path: &Path, root: &Path, home: &Path) -> bool {
    path == Path::new("/ctx/tool")
        || path == root.join("tool")
        || path == home.join("tool")
        || home
            .file_name()
            .is_some_and(|uid| path == Path::new("/ctx/home").join(uid).join("tool"))
}

fn ctx_home(root: &Path) -> Result<PathBuf, TshError> {
    if let Some(home) = env::var_os("CTX_HOME") {
        return Ok(PathBuf::from(home));
    }
    Ok(root.join("home").join(current_uid()?))
}

const ID_PROGRAM: &str = "/usr/bin/id";

fn get_id_program() -> &'static str {
    ID_PROGRAM
}

fn id_command() -> ProcessCommand {
    let mut command = ProcessCommand::new(get_id_program());
    command.arg("-u").env_clear().env("PATH", "/usr/bin:/bin");
    command
}

fn current_uid() -> Result<String, TshError> {
    let output = id_command()
        .output()
        .map_err(|error| TshError::unavailable(format!("cannot run id -u: {error}")))?;
    if !output.status.success() {
        return Err(TshError::unavailable("id -u failed"));
    }
    let uid = String::from_utf8(output.stdout)
        .map_err(|_error| TshError::unavailable("id -u returned non-UTF-8 output"))?;
    parse_current_uid(&uid)
}

fn parse_current_uid(output: &str) -> Result<String, TshError> {
    let uid = output.trim();
    if uid.is_empty() {
        return Err(TshError::unavailable("id -u returned empty output"));
    }
    if !uid.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(TshError::unavailable("id -u returned invalid uid"));
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

fn write_error_to_tsh(error: &io::Error) -> TshError {
    TshError::unavailable(format!("cannot write output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{
        LoadedTool, MAX_TSH_REPL_LINE_BYTES, ToolContext, TshCommand, TshConfig,
        append_schema_help, builtin_words, ctx_tool_path_with_home, get_id_program, help_text,
        id_command, load_tool_context, open_executable_no_follow, parse_args, parse_current_uid,
        parse_repl_line, parse_tsh_config, parse_tshrc_ctx_path, read_repl_line_canonical_from,
        read_tsh_config_text, requires_explicit_repl_input, run_repl_tool, run_tool,
        terminal_safe_text, tshrc_ctx_path, validate_tshrc_ctx_path,
    };
    use cortexfs_tool_sdk::DynamicToolCache;
    use std::ffi::OsString;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::fs::symlink;
    use std::path::Path;
    use std::path::PathBuf;
    use std::process::ExitCode;

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
    fn builtin_words_preserve_tsh_builtin_argv() {
        assert_eq!(
            builtin_words("tools", vec![OsString::from("-l")]),
            Ok(vec!["tools".to_owned(), "-l".to_owned()])
        );
    }

    #[test]
    fn help_describes_generic_visible_tool_invocation() {
        let help = help_text();

        assert!(help.contains("TOOL [ARG...]    run a visible tool with CLI-style argv and stdio"));
        assert!(!help.contains("fs.read PATH"));
    }

    #[test]
    fn get_id_program_returns_absolute_path() {
        assert_eq!(get_id_program(), "/usr/bin/id");
    }

    #[test]
    fn id_command_uses_clean_runtime_environment() {
        let command = id_command();
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let mut envs = command
            .get_envs()
            .map(|(name, value)| {
                (
                    name.to_string_lossy().into_owned(),
                    value.map(|value| value.to_string_lossy().into_owned()),
                )
            })
            .collect::<Vec<_>>();
        envs.sort();

        assert_eq!(command.get_program(), "/usr/bin/id");
        assert_eq!(args, vec!["-u".to_owned()]);
        assert_eq!(
            envs,
            vec![("PATH".to_owned(), Some("/usr/bin:/bin".to_owned()))]
        );
    }

    #[test]
    fn parse_current_uid_accepts_digits_only() {
        assert_eq!(parse_current_uid("1000\n"), Ok("1000".to_owned()));
        assert!(parse_current_uid("1000\n1001\n").is_err());
        assert!(parse_current_uid("user\n").is_err());
        assert!(parse_current_uid("\n").is_err());
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
    fn canonical_repl_reader_accepts_line_at_limit() {
        let input = format!("{}\n", "x".repeat(MAX_TSH_REPL_LINE_BYTES));
        let mut reader = std::io::Cursor::new(input);

        let line = read_repl_line_canonical_from(&mut reader);

        assert_eq!(
            line.map(|line| line.map(|line| line.len())),
            Ok(Some(MAX_TSH_REPL_LINE_BYTES))
        );
    }

    #[test]
    fn canonical_repl_reader_rejects_line_over_limit() {
        let input = format!("{}\n", "x".repeat(MAX_TSH_REPL_LINE_BYTES + 1));
        let mut reader = std::io::Cursor::new(input);

        let line = read_repl_line_canonical_from(&mut reader);

        assert!(matches!(line, Err(ref error) if error.message.contains("exceeds limit")));
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
    fn rejects_tshrc_ctx_path_outside_ctx_namespace() {
        let root = Path::new("/tmp/cortexfs-root");
        let home = root.join("home").join("1000");

        assert!(validate_tshrc_ctx_path("/ctx/tool:/ctx/home/1000/tool", root, &home).is_ok());
        assert!(
            validate_tshrc_ctx_path(
                "/tmp/cortexfs-root/tool:/tmp/cortexfs-root/home/1000/tool",
                root,
                &home,
            )
            .is_ok()
        );
        assert!(validate_tshrc_ctx_path(".", root, &home).is_err());
        assert!(validate_tshrc_ctx_path("/usr/bin", root, &home).is_err());
        assert!(validate_tshrc_ctx_path("/tmp/attacker", root, &home).is_err());
        assert!(validate_tshrc_ctx_path("/ctx/tool::/ctx/home/1000/tool", root, &home).is_err());
    }

    #[test]
    fn standalone_tshrc_ctx_path_takes_precedence_over_process_env() {
        let dir =
            std::env::temp_dir().join(format!("cortexfs-tsh-ctx-path-{}", std::process::id()));
        let root = dir.join("ctx");
        let home = root.join("home").join("1000");
        assert!(
            fs::create_dir_all(&home).is_ok(),
            "failed to create test home"
        );
        assert!(
            fs::write(
                home.join(".tshrc"),
                "CTX_PATH=/ctx/home/1000/tool:/ctx/tool\n",
            )
            .is_ok(),
            "failed to write test .tshrc"
        );

        let Ok(tool_path) = ctx_tool_path_with_home(
            &root,
            &home,
            Ok(format!(
                "{}:{}",
                root.join("tool").display(),
                home.join("tool").display()
            )),
            true,
        ) else {
            return;
        };

        assert_eq!(tool_path.dirs(), &[home.join("tool"), root.join("tool")]);

        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn standalone_tshrc_abi_paths_are_resolved_under_selected_root() {
        let dir = std::env::temp_dir().join(format!(
            "cortexfs-tsh-ctx-path-rooted-{}",
            std::process::id()
        ));
        let root = dir.join("ctx");
        let home = root.join("home").join("1000");
        assert!(fs::create_dir_all(home.join("tool")).is_ok());
        assert!(fs::create_dir_all(root.join("tool")).is_ok());
        assert!(
            fs::write(
                home.join(".tshrc"),
                "CTX_PATH=/ctx/tool:/ctx/home/1000/tool\n"
            )
            .is_ok()
        );

        let Ok(tool_path) =
            ctx_tool_path_with_home(&root, &home, Err(std::env::VarError::NotPresent), true)
        else {
            return;
        };

        assert_eq!(tool_path.dirs(), &[root.join("tool"), home.join("tool")]);
        assert!(!tool_path.dirs().contains(&PathBuf::from("/ctx/tool")));

        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn agent_tsh_process_env_takes_precedence_over_tshrc() {
        let dir = std::env::temp_dir().join(format!(
            "cortexfs-tsh-agent-ctx-path-{}",
            std::process::id()
        ));
        let root = dir.join("ctx");
        let home = root.join("home").join("1000");
        assert!(
            fs::create_dir_all(&home).is_ok(),
            "failed to create test home"
        );
        assert!(
            fs::write(
                home.join(".tshrc"),
                "CTX_PATH=/ctx/home/1000/tool:/ctx/tool\n",
            )
            .is_ok(),
            "failed to write test .tshrc"
        );

        let env_path = format!(
            "{}:{}",
            root.join("tool").display(),
            home.join("tool").display()
        );
        let Ok(tool_path) = ctx_tool_path_with_home(&root, &home, Ok(env_path), false) else {
            return;
        };

        assert_eq!(tool_path.dirs(), &[root.join("tool"), home.join("tool")]);

        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn tshrc_ctx_path_refuses_symlink() {
        let dir =
            std::env::temp_dir().join(format!("cortexfs-tshrc-symlink-{}", std::process::id()));
        let root = dir.join("ctx");
        let home = root.join("home").join("1000");
        assert!(fs::create_dir_all(&home).is_ok());
        let outside = dir.join("outside-tshrc");
        assert!(
            fs::write(&outside, "CTX_PATH=/ctx/tool\n").is_ok(),
            "failed to write outside .tshrc"
        );
        assert!(
            symlink(&outside, home.join(".tshrc")).is_ok(),
            "failed to create .tshrc symlink"
        );

        let result = tshrc_ctx_path(&root, &home);

        assert!(matches!(result, Err(error) if error.message.contains("cannot read")));
        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn tshrc_ctx_path_refuses_symlink_intermediate_directory() {
        let dir = std::env::temp_dir().join(format!(
            "cortexfs-tshrc-symlink-intermediate-{}",
            std::process::id()
        ));
        let root = dir.join("ctx");
        let outside = dir.join("outside-home");
        let home = root.join("home").join("1000");
        assert!(fs::create_dir_all(root.join("home")).is_ok());
        assert!(fs::create_dir_all(&outside).is_ok());
        assert!(fs::write(outside.join(".tshrc"), "CTX_PATH=/ctx/tool\n").is_ok());
        assert!(symlink(&outside, &home).is_ok());

        let result = tshrc_ctx_path(&root, &home);

        assert!(matches!(result, Err(error) if error.message.contains("cannot read")));
        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn parses_tsh_config_as_data() {
        assert_eq!(
            parse_tsh_config(
                "\
# tsh runtime policy
max_loaded_tools=16
cache_capacity=8
window_percent=25
"
            ),
            Ok(TshConfig {
                max_loaded_tools: 16,
                cache_capacity: 8,
                window_percent: 25,
            })
        );
        assert!(parse_tsh_config("max_loaded_tools=0\n").is_err());
        assert!(parse_tsh_config("cache_capacity=1025\n").is_err());
        assert!(parse_tsh_config("window_percent=101\n").is_err());
        assert!(parse_tsh_config("export cache_capacity=8\n").is_err());
    }

    #[test]
    fn read_tsh_config_text_refuses_symlink_config() {
        let dir = std::env::temp_dir().join(format!(
            "cortexfs-tsh-config-symlink-{}",
            std::process::id()
        ));
        let root = dir.join("ctx");
        let control_dir = root.join("tool").join("tsh.d");
        assert!(fs::create_dir_all(&control_dir).is_ok());
        let tool = root.join("tool").join("tsh");
        assert!(fs::write(&tool, "#!/bin/sh\nexit 0\n").is_ok());
        assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());
        let outside = dir.join("outside-config");
        assert!(fs::write(&outside, "max_loaded_tools=1\n").is_ok());
        assert!(symlink(&outside, control_dir.join("config")).is_ok());

        let result = read_tsh_config_text(&root);

        assert!(matches!(result, Err(error) if error.message.contains("cannot read")));
        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn open_executable_no_follow_refuses_symlink_tool() {
        let dir = std::env::temp_dir().join(format!(
            "cortexfs-tsh-executable-symlink-{}",
            std::process::id()
        ));
        assert!(fs::create_dir_all(&dir).is_ok());
        let target = dir.join("target");
        let link = dir.join("tool");
        assert!(fs::write(&target, "#!/bin/sh\nexit 0\n").is_ok());
        assert!(fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).is_ok());
        assert!(symlink(&target, &link).is_ok());

        assert!(open_executable_no_follow(&link).is_err());
        let _ignored = fs::remove_dir_all(dir);
    }

    #[test]
    fn tool_context_evicts_oldest_unpinned_tool() {
        let mut context = ToolContext::new(1);
        assert!(context.insert(test_loaded_tool("a", false)).is_empty());
        let evicted = context.insert(test_loaded_tool("b", false));
        assert_eq!(
            evicted
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["a"]
        );
        assert!(context.tools.contains_key("b"));
    }

    #[test]
    fn tool_context_touch_preserves_recently_used_tool() {
        let mut context = ToolContext::new(2);
        assert!(context.insert(test_loaded_tool("a", false)).is_empty());
        assert!(context.insert(test_loaded_tool("b", false)).is_empty());
        context.touch("a");

        let evicted = context.insert(test_loaded_tool("c", false));

        assert_eq!(
            evicted
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            vec!["b"]
        );
        assert!(context.tools.contains_key("a"));
        assert!(context.tools.contains_key("c"));
    }

    #[test]
    fn tool_context_keeps_pinned_tools_over_limit() {
        let mut context = ToolContext::new(1);
        assert!(context.insert(test_loaded_tool("a", true)).is_empty());
        assert!(context.insert(test_loaded_tool("b", false)).is_empty());
        assert!(context.tools.contains_key("a"));
        assert!(context.tools.contains_key("b"));
    }

    #[test]
    fn tool_context_reload_preserves_existing_pin() {
        let mut context = ToolContext::new(1);
        assert!(context.insert(test_loaded_tool("a", true)).is_empty());
        assert!(context.insert(test_loaded_tool("a", false)).is_empty());

        let evicted = context.insert(test_loaded_tool("b", false));

        assert!(evicted.is_empty());
        assert!(context.tools.get("a").is_some_and(|tool| tool.pinned));
        assert!(context.tools.contains_key("b"));
    }

    #[test]
    fn tool_context_unload_removes_only_unpinned_tools() {
        let mut context = ToolContext::new(2);
        assert!(context.insert(test_loaded_tool("a", true)).is_empty());
        assert!(context.insert(test_loaded_tool("b", false)).is_empty());

        assert!(context.remove_unpinned("a").is_err());
        assert!(context.tools.contains_key("a"));
        let removed = context.remove_unpinned("b");
        assert!(matches!(removed, Ok(Some(ref tool)) if tool.name == "b"));
        assert!(!context.tools.contains_key("b"));
    }

    #[test]
    fn load_tool_context_reads_metadata_without_dynamic_load() {
        let root =
            std::env::temp_dir().join(format!("cortexfs-tsh-load-context-{}", std::process::id()));
        let tool_dir = root.join("tool");
        let control_dir = root.join("tool").join("meta.d");
        assert!(fs::create_dir_all(&control_dir).is_ok());
        let tool = tool_dir.join("meta");
        assert!(fs::write(&tool, "#!/bin/sh\nexit 0\n").is_ok());
        assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());
        assert!(fs::write(control_dir.join("description"), "metadata only\n").is_ok());

        let cache = DynamicToolCache::new(4);
        let loaded = load_tool_context(&root, &cache, "meta", true);
        assert!(loaded.is_ok(), "load metadata: {loaded:?}");
        let Ok(loaded) = loaded else {
            return;
        };

        assert_eq!(loaded.name, "meta");
        assert_eq!(loaded.description, "metadata only");
        assert!(loaded.pinned);
        assert!(!loaded.dynamic_resident);
        assert!(!cache.contains_path(&tool.display().to_string()));
        assert!(!cache.is_pinned_path(&tool.display().to_string()));

        let _ignored = fs::remove_dir_all(root);
    }

    #[test]
    fn load_tool_context_ignores_symlink_metadata() {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-tsh-load-context-symlink-{}",
            std::process::id()
        ));
        let tool_dir = root.join("tool");
        let control_dir = root.join("tool").join("meta.d");
        assert!(fs::create_dir_all(&control_dir).is_ok());
        let tool = tool_dir.join("meta");
        assert!(fs::write(&tool, "#!/bin/sh\nexit 0\n").is_ok());
        assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());
        let outside = root.join("outside-description");
        assert!(fs::write(&outside, "attacker metadata\n").is_ok());
        assert!(symlink(&outside, control_dir.join("description")).is_ok());

        let cache = DynamicToolCache::new(4);
        let loaded = load_tool_context(&root, &cache, "meta", true);
        assert!(loaded.is_ok(), "load metadata: {loaded:?}");
        let Ok(loaded) = loaded else {
            return;
        };

        assert_eq!(loaded.description, "");
        let _ignored = fs::remove_dir_all(root);
    }

    #[test]
    fn load_tool_context_ignores_symlink_intermediate_metadata_dir() {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-tsh-load-context-symlink-intermediate-{}",
            std::process::id()
        ));
        let outside = root.join("outside");
        let tool_dir = root.join("tool");
        assert!(fs::create_dir_all(&tool_dir).is_ok());
        assert!(fs::create_dir_all(outside.join("meta.d")).is_ok());
        assert!(fs::write(outside.join("meta.d/description"), "attacker metadata\n").is_ok());
        let tool = tool_dir.join("meta");
        assert!(fs::write(&tool, "#!/bin/sh\nexit 0\n").is_ok());
        assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());
        assert!(symlink(&outside, tool_dir.join("meta.d")).is_ok());

        let cache = DynamicToolCache::new(4);
        let loaded = load_tool_context(&root, &cache, "meta", true);
        assert!(loaded.is_ok(), "load metadata: {loaded:?}");
        let Ok(loaded) = loaded else {
            return;
        };

        assert_eq!(loaded.description, "");
        let _ignored = fs::remove_dir_all(root);
    }

    #[test]
    fn terminal_safe_text_escapes_control_sequences() {
        assert_eq!(
            terminal_safe_text("desc-prefix-\u{1b}[31mRED\u{1b}[0m\tend"),
            "desc-prefix-\\u{1b}[31mRED\\u{1b}[0m\\tend"
        );
    }

    #[test]
    fn schema_help_escapes_decoded_control_sequences() {
        let mut text = String::new();
        append_schema_help(
            &mut text,
            r#"{
                "title":"schema-title-\u001b[35mMAGENTA\u001b[0m",
                "description":"schema-description-\u001b]52;c;AAAA\u0007",
                "required":["safe","bad\u001b[0m"]
            }"#,
        );

        assert!(!text.contains('\u{1b}'));
        assert!(!text.contains('\u{7}'));
        assert!(text.contains(r"schema-title-\u{1b}[35mMAGENTA\u{1b}[0m"));
        assert!(text.contains(r"schema-description-\u{1b}]52;c;AAAA\u{7}"));
        assert!(text.contains(r"required: safe bad\u{1b}[0m"));
    }

    #[test]
    fn tsh_refuses_tool_execution_without_agent_authority() {
        let root =
            std::env::temp_dir().join(format!("cortexfs-tsh-empty-argv-{}", std::process::id()));
        let tool_dir = root.join("tool");
        assert!(fs::create_dir_all(&tool_dir).is_ok());
        let tool = tool_dir.join("noop");
        assert!(fs::write(&tool, "#!/bin/sh\n[ \"$CTX_TOOL_MODE\" = cli ]\n").is_ok());
        assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());

        let result = run_tool(&root, "noop", Vec::new());
        assert!(matches!(
            result,
            Err(error)
                if error.message.contains("CTX_AGENT")
                    && error.message.contains("ctx agent attach AGENT")
        ));
        let _ignored = fs::remove_dir_all(root);
    }

    #[test]
    fn tsh_tool_execution_gets_clean_agent_environment() {
        if std::env::var_os("CORTEXFS_TSH_ENV_CHILD").is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap_or_default())
                .arg("--exact")
                .arg("tests::tsh_tool_execution_gets_clean_agent_environment")
                .arg("--nocapture")
                .env("CORTEXFS_TSH_ENV_CHILD", "1")
                .env("CORTEXFS_SHOULD_NOT_LEAK", "secret")
                .env("CTX_AGENT", "coder")
                .output();
            assert!(matches!(output, Ok(ref output) if output.status.success()));
            return;
        }

        let root = std::env::temp_dir().join(format!(
            "cortexfs-tsh-clean-tool-env-{}",
            std::process::id()
        ));
        let control = root.join("agent").join("coder.d");
        let tool_control = root.join("tool").join("probe.d");
        assert!(fs::create_dir_all(&control).is_ok());
        assert!(fs::create_dir_all(&tool_control).is_ok());
        assert!(fs::write(control.join("owner"), "1000\n").is_ok());
        assert!(fs::write(control.join("uid"), "1000\n").is_ok());
        assert!(fs::write(control.join("gid"), "1000\n").is_ok());
        assert!(fs::write(control.join("groups"), "1000\n").is_ok());
        assert!(fs::write(control.join("label"), "user_u:agent_r:coder_t:s0\n").is_ok());
        assert!(fs::write(control.join("iso"), "shared\n").is_ok());
        assert!(fs::write(control.join("parent"), "\n").is_ok());
        assert!(fs::write(control.join("life"), "owned\n").is_ok());
        assert!(fs::write(control.join("root"), "/ctx/home/1000/agent/coder/root\n").is_ok());
        assert!(fs::write(control.join("cwd"), "/workspace\n").is_ok());
        assert!(fs::write(control.join("env"), "\n").is_ok());
        assert!(fs::write(control.join("model"), "main\n").is_ok());
        assert!(fs::write(control.join("status"), "idle\n").is_ok());
        assert!(fs::write(control.join("pid"), "\n").is_ok());
        assert!(fs::write(control.join("log"), "\n").is_ok());
        assert!(fs::write(control.join("meta.json"), "{}\n").is_ok());
        assert!(
            fs::write(
                control.join("path"),
                format!("{}\n", root.join("tool").display())
            )
            .is_ok()
        );
        assert!(
            fs::write(
                control.join("mount"),
                format!(
                    "{}\t{}\tro\trbind,nosuid,nodev\n",
                    root.display(),
                    root.display()
                ),
            )
            .is_ok()
        );
        assert!(
            fs::write(
                control.join("policy"),
                "allow coder_t model:main use\nallow coder_t tool:probe execute\n",
            )
            .is_ok()
        );
        assert!(
            fs::write(
                tool_control.join("policy"),
                "allow coder_t tool:probe execute\n"
            )
            .is_ok()
        );
        let tool = root.join("tool").join("probe");
        assert!(
            fs::write(
                &tool,
                r#"#!/bin/sh
[ -z "$CORTEXFS_SHOULD_NOT_LEAK" ] || exit 10
[ "$CTX_TOOL_MODE" = cli ] || exit 11
[ "$CTX_AGENT" = coder ] || exit 12
[ "$CTX_AUTHORIZED_OBJECT" = /ctx/tool/probe ] || exit 15
[ "$PATH" = /usr/bin:/bin ] || exit 13
[ -n "$CTX_ROOT" ] || exit 14
exit 0
"#,
            )
            .is_ok()
        );
        assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());

        let result = run_tool(&root, "probe", Vec::new());

        assert!(matches!(result, Ok(code) if code == ExitCode::SUCCESS));
        let _ignored = fs::remove_dir_all(root);
    }

    #[test]
    fn repl_allows_empty_argv_for_normal_cli_tools() {
        let root = std::env::temp_dir().join(format!(
            "cortexfs-tsh-repl-empty-normal-{}",
            std::process::id()
        ));
        let tool_dir = root.join("tool");
        assert!(fs::create_dir_all(&tool_dir).is_ok());
        let tool = tool_dir.join("noop");
        assert!(fs::write(&tool, "#!/bin/sh\nexit 0\n").is_ok());
        assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());

        let mut context = ToolContext::new(4);
        let result = run_repl_tool(&root, &mut context, "noop", Vec::new());

        assert!(matches!(
            result,
            Err(error)
                if error.message.contains("CTX_AGENT")
                    && error.message.contains("ctx agent attach AGENT")
        ));
        let _ignored = fs::remove_dir_all(root);
    }

    #[test]
    fn repl_keeps_explicit_input_guard_for_structured_core_tools() {
        assert!(requires_explicit_repl_input("fs.read"));
        assert!(requires_explicit_repl_input("fs.write"));
        assert!(requires_explicit_repl_input("shell.exec"));
        assert!(!requires_explicit_repl_input("ls"));
        assert!(!requires_explicit_repl_input("project.test"));
    }

    fn test_loaded_tool(name: &str, pinned: bool) -> LoadedTool {
        LoadedTool {
            name: name.to_owned(),
            path: PathBuf::from(format!("/ctx/tool/{name}")),
            description: String::new(),
            schema: None,
            dynamic_resident: false,
            pinned,
            last_used: 0,
        }
    }
}
