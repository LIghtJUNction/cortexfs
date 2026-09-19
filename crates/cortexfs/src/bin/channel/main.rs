#![forbid(unsafe_code)]
#![expect(clippy::allow_attributes, reason = "target-specific lint exceptions")]
#![allow(unfulfilled_lint_expectations, reason = "target-specific lint results")]

pub mod catalog;
pub mod commands;
pub mod config;
pub mod gmail;
pub mod web;
pub mod webhook;

use std::io::Write as _;
use std::process::ExitCode;

fn bearer_authorized(value: Option<&str>, token: Option<&str>) -> bool {
    let Some(token) = token else {
        return true;
    };
    let Some((scheme, credential)) = value.and_then(|value| value.split_once(' ')) else {
        return false;
    };
    let credential = credential.trim_start_matches(' ');
    !credential.is_empty() && scheme.eq_ignore_ascii_case("bearer") && credential == token
}

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
    use super::{bearer_authorized, install_crypto_provider};

    #[test]
    fn bearer_auth_follows_http_scheme_rules() {
        assert!(bearer_authorized(None, None));
        assert!(bearer_authorized(Some("Bearer secret"), Some("secret")));
        assert!(bearer_authorized(Some("bEaReR secret"), Some("secret")));
        assert!(bearer_authorized(Some("BEARER   secret"), Some("secret")));
        assert!(!bearer_authorized(None, Some("secret")));
        assert!(!bearer_authorized(Some("Basic secret"), Some("secret")));
        assert!(!bearer_authorized(Some("Bearer wrong"), Some("secret")));
        assert!(!bearer_authorized(Some("Bearer "), Some("")));
    }

    #[test]
    fn channel_startup_installs_rustls_provider() {
        install_crypto_provider();
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }
}
