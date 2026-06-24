use std::env;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::io::{self, Write};
use std::process::ExitCode;
use std::thread;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;
const DEFAULT_SHELL: &str = "tsh";

#[derive(Debug, Eq, PartialEq)]
struct CtxtermError {
    code: u8,
    message: String,
}

impl CtxtermError {
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
            let _ignored = write_error(&format!("ctxterm: {}", error.message));
            ExitCode::from(error.code)
        }
    }
}

fn run(args: Vec<OsString>) -> Result<ExitCode, CtxtermError> {
    let command = parse_args(args)?;
    match command {
        CtxtermCommand::Help => print_help().map(|()| ExitCode::SUCCESS),
        CtxtermCommand::Run { program, args } => run_pty(&program, args),
    }
}

#[derive(Debug, Eq, PartialEq)]
enum CtxtermCommand {
    Help,
    Run {
        program: OsString,
        args: Vec<OsString>,
    },
}

fn parse_args(args: Vec<OsString>) -> Result<CtxtermCommand, CtxtermError> {
    let mut values = args.into_iter();
    let Some(first) = values.next() else {
        return Ok(CtxtermCommand::Run {
            program: OsString::from(DEFAULT_SHELL),
            args: Vec::new(),
        });
    };
    if first == "--help" || first == "-h" {
        return Ok(CtxtermCommand::Help);
    }
    if first == "--" {
        let Some(program) = values.next() else {
            return Err(CtxtermError::usage("-- requires a command"));
        };
        return Ok(CtxtermCommand::Run {
            program,
            args: values.collect(),
        });
    }
    Ok(CtxtermCommand::Run {
        program: first,
        args: values.collect(),
    })
}

fn print_help() -> Result<(), CtxtermError> {
    write_stdout(
        "\
ctxterm - CortexFS agent terminal emulator

usage:
  ctxterm
  ctxterm -- COMMAND [ARG...]

default:
  ctxterm starts tsh
",
    )
}

fn run_pty(program: &OsStr, args: Vec<OsString>) -> Result<ExitCode, CtxtermError> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(pty_size())
        .map_err(|error| CtxtermError::unavailable(format!("cannot open pty: {error}")))?;
    let mut command = CommandBuilder::new(program);
    let cwd = env::current_dir().map_err(|error| {
        CtxtermError::unavailable(format!("cannot read current directory: {error}"))
    })?;
    command.cwd(cwd.as_os_str());
    command.args(args);
    let mut child = pair
        .slave
        .spawn_command(command)
        .map_err(|error| CtxtermError::unavailable(format!("cannot run command: {error}")))?;
    drop(pair.slave);

    let mut reader = pair
        .master
        .try_clone_reader()
        .map_err(|error| CtxtermError::unavailable(format!("cannot open pty reader: {error}")))?;
    let mut writer = pair
        .master
        .take_writer()
        .map_err(|error| CtxtermError::unavailable(format!("cannot open pty writer: {error}")))?;

    let output = thread::spawn(move || {
        let mut stdout = io::stdout().lock();
        io::copy(&mut reader, &mut stdout).and_then(|_bytes| stdout.flush())
    });
    let _input = thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        io::copy(&mut stdin, &mut writer)
    });

    let status = child
        .wait()
        .map_err(|error| CtxtermError::unavailable(format!("cannot wait for command: {error}")))?;
    match output.join() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(write_error_to_ctxterm(&error)),
        Err(_error) => return Err(CtxtermError::unavailable("pty output thread failed")),
    }
    Ok(exit_code(&status))
}

fn pty_size() -> PtySize {
    PtySize {
        rows: env_u16("LINES").unwrap_or(DEFAULT_ROWS),
        cols: env_u16("COLUMNS").unwrap_or(DEFAULT_COLS),
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn env_u16(name: &str) -> Option<u16> {
    env::var(name).ok()?.parse::<u16>().ok()
}

fn exit_code(status: &portable_pty::ExitStatus) -> ExitCode {
    u8::try_from(status.exit_code()).map_or_else(|_error| ExitCode::from(1), ExitCode::from)
}

fn write_stdout(message: &str) -> Result<(), CtxtermError> {
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(message.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|error| write_error_to_ctxterm(&error))
}

fn write_error(message: &str) -> io::Result<()> {
    let mut stderr = io::stderr().lock();
    writeln!(stderr, "{message}")
}

fn write_error_to_ctxterm(error: &io::Error) -> CtxtermError {
    CtxtermError::unavailable(format!("cannot write output: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{CtxtermCommand, parse_args};
    use std::ffi::OsString;

    #[test]
    fn ctxterm_defaults_to_tsh() {
        assert_eq!(
            parse_args(Vec::new()),
            Ok(CtxtermCommand::Run {
                program: OsString::from("tsh"),
                args: Vec::new()
            })
        );
    }

    #[test]
    fn ctxterm_accepts_explicit_command_after_separator() {
        assert_eq!(
            parse_args(vec![
                OsString::from("--"),
                OsString::from("tsh"),
                OsString::from("--list"),
            ]),
            Ok(CtxtermCommand::Run {
                program: OsString::from("tsh"),
                args: vec![OsString::from("--list")]
            })
        );
    }
}
