#![forbid(unsafe_code)]

pub mod catalog;
pub mod commands;
pub mod config;
pub mod gmail;
pub mod web;
pub mod webhook;

use std::io::Write as _;
use std::process::ExitCode;

fn main() -> ExitCode {
    install_crypto_provider();
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = writeln!(std::io::stderr(), "cortexfs-channel: {error}");
            ExitCode::from(1)
        }
    }
}

fn install_crypto_provider() {
    let _ignored = rustls::crypto::ring::default_provider().install_default();
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    commands::run(config::load()?)
}

#[cfg(test)]
mod tests {
    use super::install_crypto_provider;

    #[test]
    fn channel_startup_installs_rustls_provider() {
        install_crypto_provider();
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }
}
