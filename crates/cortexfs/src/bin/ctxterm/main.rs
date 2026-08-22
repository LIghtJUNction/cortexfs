#![forbid(unsafe_code)]
#![expect(
    clippy::allow_attributes,
    reason = "allow target-specific lint exceptions"
)]
#![allow(
    unfulfilled_lint_expectations,
    reason = "expected target-specific lint results"
)]
#![expect(clippy::wildcard_imports, reason = "uniform submodule imports")]
#![expect(clippy::redundant_pub_crate, reason = "submodule visibility alignment")]
#![expect(clippy::module_inception, reason = "allow submodule self name")]

use std::env;
use std::ffi::{OsStr, OsString};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use cortexfs::define_simple_cli_error;
use portable_pty::{CommandBuilder, PtySize, native_pty_system};

const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;
const DEFAULT_SHELL: &str = cortexfs::support::command::TSH;
const MAX_CLIENTS: usize = 16;
const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_secs(1);
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

pub(crate) use cortexfs::cli::stderr;

define_simple_cli_error!(CtxtermError);

pub(crate) use cli::*;
pub(crate) use client::*;
pub(crate) use pty::*;
pub(crate) use socket::*;
pub(crate) use stderr::*;

pub(crate) fn main() -> ExitCode {
    run(env::args_os().skip(1).collect()).unwrap_or_else(|error| {
        let _ignored = write_error(&format!("ctxterm: {}", error.message));
        ExitCode::from(error.code)
    })
}

pub(crate) fn run(args: Vec<OsString>) -> Result<ExitCode, CtxtermError> {
    match parse_args(args)? {
        CtxtermCommand::Help => print_help().map(|()| ExitCode::SUCCESS),
        CtxtermCommand::Run {
            broker,
            program,
            args,
        } => run_pty(&RunConfig {
            broker,
            program,
            args,
        }),
    }
}

#[derive(Debug, Eq, PartialEq)]
enum CtxtermCommand {
    Help,
    Run {
        broker: BrokerConfig,
        program: OsString,
        args: Vec<OsString>,
    },
}

#[derive(Debug, Eq, PartialEq)]
struct BrokerConfig {
    agent: String,
    session: String,
    unit: String,
}

struct RunConfig {
    broker: BrokerConfig,
    program: OsString,
    args: Vec<OsString>,
}

pub mod cli;
pub mod client;
pub mod pty;
pub mod socket;

#[cfg(test)]
pub mod tests;
