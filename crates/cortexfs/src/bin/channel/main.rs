#![forbid(unsafe_code)]

pub mod commands;
pub mod config;
pub mod gmail;
pub mod web;
pub mod webhook;

use std::io::Write as _;
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = writeln!(std::io::stderr(), "cortexfs-channel: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    commands::run(config::load()?)
}
