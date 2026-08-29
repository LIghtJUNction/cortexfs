#![forbid(unsafe_code)]
#![expect(
    clippy::redundant_pub_crate,
    clippy::field_scoped_visibility_modifiers,
    reason = "private adapter modules share narrow binary state"
)]

mod args;
mod client;
mod http;
mod registry;
mod trajectory;

use std::env;
use std::fmt;
use std::io::{self, Write};
use std::process::ExitCode;

pub(crate) type AppResult<T> = Result<T, AppError>;

#[derive(Debug)]
pub(crate) struct AppError(String);

impl AppError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for AppError {}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = writeln!(io::stderr(), "cortexfs-futureagi: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> AppResult<()> {
    let command = args::parse(env::args().skip(1))?;
    match command {
        args::Command::Export(options) => {
            let trajectory = trajectory::load(&options.trajectory)?;
            let cases = trajectory::cases(&trajectory, options.include_context);
            write_json(&cases)
        }
        args::Command::Evaluate(options) => {
            let trajectory = trajectory::load(&options.trajectory)?;
            let cases = trajectory::cases(&trajectory, options.include_context);
            let response = client::evaluate(&options, &cases)?;
            write_json(&response)
        }
    }
}

fn write_json<T: serde::Serialize>(value: &T) -> AppResult<()> {
    let mut output = serde_json::to_vec_pretty(value)
        .map_err(|error| AppError::new(format!("cannot encode JSON: {error}")))?;
    output.push(b'\n');
    io::stdout()
        .write_all(&output)
        .map_err(|error| AppError::new(format!("cannot write stdout: {error}")))
}
