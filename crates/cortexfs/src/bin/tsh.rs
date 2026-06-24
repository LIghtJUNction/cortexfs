use std::env;
use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode, Stdio};

use cortexfs::{CTX_ROOT, ToolPath};

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
    match command {
        TshCommand::Help => print_help().map(|()| ExitCode::SUCCESS),
        TshCommand::List => list_tools(&root).map(|()| ExitCode::SUCCESS),
        TshCommand::Repl => run_repl(&root),
        TshCommand::Tool { name, args } => run_tool(&root, &name, args),
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
  tsh never falls back to PATH for tool lookup
  bash, tmux, and zellij are tools, not built-ins
",
    )
}

fn list_tools(root: &Path) -> Result<(), TshError> {
    let tool_path = ctx_tool_path(root)?;
    let hits = tool_path.list().map_err(tool_path_error)?;
    let mut stdout = io::stdout().lock();
    for hit in hits {
        let Some(name) = hit.path().file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        writeln!(stdout, "{name}").map_err(|error| write_error_to_tsh(&error))?;
    }
    stdout.flush().map_err(|error| write_error_to_tsh(&error))
}

fn run_repl(root: &Path) -> Result<ExitCode, TshError> {
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
        let words = parse_repl_line(&line)?;
        if words.is_empty() {
            continue;
        }
        match words.first().map(String::as_str) {
            Some("exit" | "quit") => return parse_exit_code(&words),
            Some("help") => print_help()?,
            Some("tools") => list_tools(root)?,
            Some(name) => {
                let args = words.iter().skip(1).map(OsString::from).collect::<Vec<_>>();
                let _code = run_tool(root, name, args)?;
            }
            None => {}
        }
    }
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
    let tool_path = ctx_tool_path(root)?;
    let Some(hit) = tool_path.find(name).map_err(tool_path_error)? else {
        return Err(TshError::unavailable(format!(
            "tool not found in CTX_PATH: {name}"
        )));
    };
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

fn ctx_tool_path(root: &Path) -> Result<ToolPath, TshError> {
    match env::var("CTX_PATH") {
        Ok(value) => Ok(ToolPath::parse(&value)),
        Err(env::VarError::NotPresent) => Ok(ToolPath::default(root, &ctx_home(root)?)),
        Err(env::VarError::NotUnicode(_value)) => Err(TshError::usage("CTX_PATH must be UTF-8")),
    }
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

fn read_error_to_tsh(error: &io::Error) -> TshError {
    TshError::unavailable(format!("cannot read input: {error}"))
}

fn write_error_to_tsh(error: &io::Error) -> TshError {
    TshError::unavailable(format!("cannot write output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{TshCommand, parse_args, parse_repl_line};
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
}
