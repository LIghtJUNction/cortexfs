use std::io;

use crate::{CliError, is_provider_name, print_line};
use cortexfs::cli::input::read_limited_input_text;

pub(crate) const MAX_PROVIDER_SECRET_STDIN_BYTES: usize = 8 * 1024;

pub(crate) fn provider_secret_set(provider: &str, slot: &str) -> Result<(), CliError> {
    validate_provider_secret_target(provider, slot)?;
    let secret = read_limited_input_text(
        io::stdin(),
        MAX_PROVIDER_SECRET_STDIN_BYTES,
        "provider secret stdin exceeds limit",
    )
    .map_err(|error| CliError::unavailable(format!("cannot read secret from stdin: {error}")))?;
    let secret = secret.trim_end_matches(['\r', '\n']);
    if secret.is_empty() {
        return Err(CliError::usage(
            "provider secret set reads a non-empty secret from stdin",
        ));
    }
    cortexfs::store_provider_system_secret(provider, slot, secret)
        .map_err(provider_system_secret_cli_error)?;
    print_line(&format!("provider secret configured: {provider}/{slot}"))
}

pub(crate) fn provider_secret_status(provider: &str, slot: &str) -> Result<(), CliError> {
    validate_provider_secret_target(provider, slot)?;
    let configured = cortexfs::provider_system_secret_exists(provider, slot)
        .map_err(provider_system_secret_cli_error)?;
    print_line(&format!(
        "provider secret {provider}/{slot}: {}",
        if configured { "configured" } else { "missing" }
    ))
}

pub(crate) fn validate_provider_secret_target(provider: &str, slot: &str) -> Result<(), CliError> {
    if !is_provider_name(provider) {
        return Err(CliError::usage("invalid provider name"));
    }
    if !is_provider_secret_slot(slot) {
        return Err(CliError::usage("invalid provider secret slot"));
    }
    Ok(())
}

pub(crate) fn is_provider_secret_slot(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

pub(crate) fn provider_system_secret_cli_error(
    error: cortexfs::ProviderSystemSecretError,
) -> CliError {
    match error {
        cortexfs::ProviderSystemSecretError::InvalidName => {
            CliError::usage("invalid provider secret name")
        }
        cortexfs::ProviderSystemSecretError::CannotRead => {
            CliError::unavailable("cannot read provider system secret")
        }
        cortexfs::ProviderSystemSecretError::CannotWrite => CliError::unavailable(
            "cannot write provider system secret; run with sudo or install via a privileged helper",
        ),
    }
}
