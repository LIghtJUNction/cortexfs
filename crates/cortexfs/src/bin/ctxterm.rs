#![forbid(unsafe_code)]

use std::env;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, IsTerminal, Read, Write};
use std::net::Shutdown;
use std::os::fd::AsFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use nix::sys::termios::{SetArg, Termios, cfmakeraw, tcgetattr, tcsetattr};
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;
const DEFAULT_SHELL: &str = "/usr/bin/tsh";
const CLIENT_MODE_LIMIT: usize = 16;
const CLIENT_MODE_TIMEOUT: Duration = Duration::from_secs(1);
const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_secs(1);
const MAX_EMIT_PAYLOAD_BYTES: usize = 64 * 1024;
const PRESERVED_PTY_ENV: &[&str] = &[
    "CTX_ROOT",
    "CTX_HOME",
    "CTX_AGENT",
    "CTX_AGENT_SUBJECT",
    "CTX_PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
];

type PtyWriter = Arc<Mutex<Box<dyn Write + Send>>>;
type Client = Arc<Mutex<UnixStream>>;
type Clients = Arc<Mutex<Vec<Client>>>;

include!("shared/stderr.rs");
include!("shared/terminal_io.rs");
include!("shared/simple_cli_error.rs");

define_simple_cli_error!(CtxtermError);

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
        CtxtermCommand::Run {
            listen,
            log,
            stdio,
            program,
            args,
        } => run_pty(RunConfig {
            listen,
            log,
            stdio,
            program,
            args,
        }),
        CtxtermCommand::Client { socket, write } => run_client(&socket, write),
    }
}

#[derive(Debug, Eq, PartialEq)]
enum CtxtermCommand {
    Help,
    Run {
        listen: Option<PathBuf>,
        log: Option<PathBuf>,
        stdio: bool,
        program: OsString,
        args: Vec<OsString>,
    },
    Client {
        socket: PathBuf,
        write: bool,
    },
}

struct RunConfig {
    listen: Option<PathBuf>,
    log: Option<PathBuf>,
    stdio: bool,
    program: OsString,
    args: Vec<OsString>,
}

include!("ctxterm/cli.rs");
include!("ctxterm/pty.rs");
include!("shared/plain_dir.rs");
include!("shared/create_plain_dir.rs");
include!("shared/stale_socket.rs");
include!("ctxterm/socket.rs");
include!("ctxterm/fs.rs");
include!("ctxterm/client_io.rs");

#[cfg(test)]
include!("ctxterm/tests.rs");
