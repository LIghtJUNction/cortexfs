pub mod clipboard;
pub mod complete;
pub mod protocol;
pub mod reference;
pub mod render;

use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use complete::ChatHelper;
use rustyline::Editor;
use rustyline::history::DefaultHistory;

struct Options {
    root: PathBuf,
    agent: String,
    session: String,
    raw: bool,
    approvals: Vec<String>,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = writeln!(io::stderr().lock(), "ctxchat: {error}");
            ExitCode::from(69)
        }
    }
}

fn run() -> io::Result<()> {
    let mut options = parse(env::args().skip(1))?;
    let socket = cortexfs_paths::agent_socket_path(&options.root, &options.agent);
    let workspace = env::current_dir()?;
    if !io::stdin().is_terminal() {
        let mut input = String::new();
        io::stdin().take(1024 * 1024).read_to_string(&mut input)?;
        if !input.is_empty() {
            send_text(
                &socket,
                &options,
                &reference::expand(&input, &workspace, &messages(&options))?,
            )?;
        }
        return Ok(());
    }
    let mut editor = Editor::<ChatHelper, DefaultHistory>::with_config(
        rustyline::Config::builder()
            .enable_signals(true)
            .bracketed_paste(true)
            .build(),
    )
    .map_err(io::Error::other)?;
    editor.set_helper(Some(helper(&options, &workspace)));
    if io::stdin().is_terminal() {
        banner(&options, &workspace);
    }
    loop {
        let prompt = format!("ctxchat {}/{} ❯ ", options.agent, options.session);
        let line = match editor.readline(&prompt) {
            Ok(line) => line,
            Err(
                rustyline::error::ReadlineError::Interrupted | rustyline::error::ReadlineError::Eof,
            ) => return Ok(()),
            Err(error) => return Err(io::Error::other(error)),
        };
        if line.trim().is_empty() {
            continue;
        }
        let _ignored = editor.add_history_entry(line.as_str());
        match line.as_str() {
            "/exit" | "/quit" => return Ok(()),
            "/help" => {
                banner(&options, &workspace);
            }
            command if command.split_whitespace().next() == Some("/raw") => {
                options.raw = raw_mode(command, options.raw)?;
                writeln!(
                    io::stderr().lock(),
                    "ctxchat: raw={}",
                    if options.raw { "on" } else { "off" }
                )?;
            }
            "/clear" => {
                render::clear()?;
            }
            "/history" => {
                print_file(&messages(&options))?;
            }
            "/output" => {
                print_file(&session_dir(&options).join("latest.md"))?;
            }
            "/status" => {
                print_file(&session_dir(&options).join("state"))?;
            }
            "/tools" => {
                for tool in tool_names(&options.root) {
                    writeln!(
                        io::stdout().lock(),
                        "{}",
                        cortexfs::support::terminal::terminal_safe_text(&tool)
                    )?;
                }
            }
            "/paste" => send_text(
                &socket,
                &options,
                &reference::expand(&clipboard::read()?, &workspace, &messages(&options))?,
            )?,
            command if command.starts_with("/copy") => {
                copy(&options, command)?;
            }
            command if command.starts_with("/new") => {
                let next = command
                    .split_whitespace()
                    .nth(1)
                    .map_or_else(request_id, str::to_owned);
                validate_name(&next)?;
                options.session = next;
                editor.set_helper(Some(helper(&options, &workspace)));
            }
            command if command.starts_with(':') => {
                let args = command
                    .strip_prefix(':')
                    .unwrap_or("")
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                if args.is_empty() {
                    continue;
                }
                render::frames(
                    &protocol::tsh(&socket, &request_id(), &options.session, &args)?,
                    options.raw,
                )?;
            }
            command if command.starts_with('/') => {
                writeln!(
                    io::stderr().lock(),
                    "ctxchat: unsupported command {command}"
                )?;
            }
            _ => send_text(
                &socket,
                &options,
                &reference::expand(&line, &workspace, &messages(&options))?,
            )?,
        }
    }
}

fn send_text(socket: &Path, options: &Options, text: &str) -> io::Result<()> {
    render::frames(
        &protocol::send(
            socket,
            &request_id(),
            &options.session,
            text,
            &options.approvals,
        )?,
        options.raw,
    )
}

fn copy(options: &Options, command: &str) -> io::Result<()> {
    let history = reference::history_texts(&messages(options))?;
    let index = command
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<usize>().ok());
    let text = index
        .and_then(|index| history.get(index))
        .or_else(|| history.last())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no message to copy"))?;
    clipboard::write(text)
}

fn helper(options: &Options, workspace: &Path) -> ChatHelper {
    ChatHelper {
        workspace: workspace.to_path_buf(),
        messages: messages(options),
        tools: tool_names(&options.root),
    }
}

fn session_dir(options: &Options) -> PathBuf {
    let uid = nix::unistd::Uid::effective().as_raw();
    cortexfs_paths::agent_session_path(
        &options.root,
        &uid.to_string(),
        &options.agent,
        &options.session,
    )
}
fn messages(options: &Options) -> PathBuf {
    session_dir(options).join("messages.jsonl")
}
fn tool_names(root: &Path) -> Vec<String> {
    const MAX_TOOLS: usize = 4096;
    fs::read_dir(cortexfs_paths::tool_root_path(root))
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| {
            Path::new(name)
                .extension()
                .is_none_or(|ext| !ext.eq_ignore_ascii_case("d"))
        })
        .take(MAX_TOOLS)
        .collect()
}
fn print_file(path: &Path) -> io::Result<()> {
    match fs::read_to_string(path) {
        Ok(text) => {
            io::stdout()
                .lock()
                .write_all(cortexfs::support::terminal::terminal_safe_text(&text).as_bytes())?;
            Ok(())
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
fn request_id() -> String {
    format!(
        "chat-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
    )
}
fn validate_name(value: &str) -> io::Result<()> {
    if !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        Ok(())
    } else {
        Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid name"))
    }
}

fn parse(args: impl Iterator<Item = String>) -> io::Result<Options> {
    let mut root = cortexfs_paths::ctx_root();
    let mut agent = None;
    let mut session = "default".to_owned();
    let mut raw = false;
    let mut approvals = Vec::new();
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                root = PathBuf::from(args.next().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--root requires path")
                })?);
            }
            "--session" => {
                session = args.next().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--session requires name")
                })?;
            }
            "--raw" => raw = true,
            "--approval" => {
                let approval = args.next().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "--approval requires tool name")
                })?;
                if !cortexfs::is_object_name(&approval) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "invalid approved tool name",
                    ));
                }
                approvals.push(approval);
            }
            value if !value.starts_with('-') && agent.is_none() => agent = Some(value.to_owned()),
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unknown argument {arg}"),
                ));
            }
        }
    }
    let agent =
        agent.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "agent name required"))?;
    validate_name(&agent)?;
    validate_name(&session)?;
    Ok(Options {
        root,
        agent,
        session,
        raw,
        approvals,
    })
}

fn banner(options: &Options, workspace: &Path) {
    let _ignored = writeln!(
        io::stderr().lock(),
        "ctxchat {}/{}  workspace={}  raw={}\n/help /raw [on|off] /new /history /output /tools /status /paste /copy /clear /exit | :load :pin :loads | @path @history:N",
        options.agent,
        options.session,
        workspace.display(),
        if options.raw { "on" } else { "off" }
    );
}

fn raw_mode(command: &str, current: bool) -> io::Result<bool> {
    let mut parts = command.split_whitespace();
    if parts.next() != Some("/raw") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid /raw command",
        ));
    }
    let next = parts.next();
    if parts.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: /raw [on|off]",
        ));
    }
    match next {
        None | Some("toggle") => Ok(!current),
        Some("on") => Ok(true),
        Some("off") => Ok(false),
        Some(_) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "usage: /raw [on|off]",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_preserves_repeatable_approval_policy() -> io::Result<()> {
        let options = parse(
            [
                "coder",
                "--session",
                "work",
                "--approval",
                "example.echo",
                "--approval",
                "fs.read",
            ]
            .into_iter()
            .map(str::to_owned),
        )?;
        assert_eq!(options.approvals, ["example.echo", "fs.read"]);
        Ok(())
    }

    #[test]
    fn raw_command_toggles_or_selects_mode() -> io::Result<()> {
        assert!(raw_mode("/raw", false)?);
        assert!(!raw_mode("/raw", true)?);
        assert!(raw_mode("/raw on", false)?);
        assert!(!raw_mode("/raw off", true)?);
        assert!(raw_mode("/raw nope", false).is_err());
        Ok(())
    }
}
