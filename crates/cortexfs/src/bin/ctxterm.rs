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

use std::env;
use std::ffi::OsStr;
use std::ffi::OsString;
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

#[path = "shared/stderr.rs"]
pub mod stderr;
#[path = "shared/terminal-io.rs"]
pub mod terminal_io;
#[macro_use]
#[path = "shared/simple-cli-error.rs"]
pub mod simple_cli_error;

define_simple_cli_error!(CtxtermError);

pub(crate) use cli::*;
pub(crate) use client_io::*;
pub(crate) use cortexfs::plain_fs::open_plain_directory;
pub(crate) use create_plain_dir::*;
pub(crate) use fs::*;
pub(crate) use pty::*;
pub(crate) use socket::*;
pub(crate) use stale_socket::*;
pub(crate) use stderr::*;
pub(crate) use terminal_io::*;

pub(crate) fn main() -> ExitCode {
    match run(env::args_os().skip(1).collect()) {
        Ok(code) => code,
        Err(error) => {
            let _ignored = write_error(&format!("ctxterm: {}", error.message));
            ExitCode::from(error.code)
        }
    }
}

pub(crate) fn run(args: Vec<OsString>) -> Result<ExitCode, CtxtermError> {
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

#[path = "ctxterm/cli.rs"]
pub mod cli;
#[path = "ctxterm/client-io.rs"]
pub mod client_io;
#[path = "shared/create-plain-dir.rs"]
pub mod create_plain_dir;
#[path = "ctxterm/fs.rs"]
pub mod fs;
#[path = "shared/plain-dir.rs"]
pub mod plain_dir;
#[path = "ctxterm/pty.rs"]
pub mod pty;
#[path = "ctxterm/socket.rs"]
pub mod socket;
#[path = "shared/stale-socket.rs"]
pub mod stale_socket;

#[cfg(test)]
#[path = "ctxterm/tests.rs"]
pub mod tests;
