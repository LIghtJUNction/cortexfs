const MAX_PROVIDER_SECRET_STDIN_BYTES: usize = 8 * 1024;

fn provider_secret_set(provider: &str, slot: &str) -> Result<(), CliError> {
    validate_provider_secret_target(provider, slot)?;
    let secret = read_provider_secret_stdin_limited(io::stdin(), MAX_PROVIDER_SECRET_STDIN_BYTES)
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

fn read_provider_secret_stdin_limited(
    reader: impl Read,
    max_bytes: usize,
) -> io::Result<String> {
    let limit = u64::try_from(max_bytes.saturating_add(1)).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("stdin read limit is invalid: {error}"),
        )
    })?;
    let mut secret = String::new();
    reader.take(limit).read_to_string(&mut secret)?;
    if secret.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "provider secret stdin exceeds limit",
        ));
    }
    Ok(secret)
}

fn provider_secret_status(provider: &str, slot: &str) -> Result<(), CliError> {
    validate_provider_secret_target(provider, slot)?;
    let configured = cortexfs::provider_system_secret_exists(provider, slot)
        .map_err(provider_system_secret_cli_error)?;
    print_line(&format!(
        "provider secret {provider}/{slot}: {}",
        if configured { "configured" } else { "missing" }
    ))
}

fn validate_provider_secret_target(provider: &str, slot: &str) -> Result<(), CliError> {
    if !is_provider_name(provider) {
        return Err(CliError::usage("invalid provider name"));
    }
    if !is_provider_secret_slot(slot) {
        return Err(CliError::usage("invalid provider secret slot"));
    }
    Ok(())
}

fn is_provider_secret_slot(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn provider_system_secret_cli_error(error: cortexfs::ProviderSystemSecretError) -> CliError {
    match error {
        cortexfs::ProviderSystemSecretError::InvalidName => CliError::usage("invalid provider secret name"),
        cortexfs::ProviderSystemSecretError::CannotRead => {
            CliError::unavailable("cannot read provider system secret")
        }
        cortexfs::ProviderSystemSecretError::CannotWrite => CliError::unavailable(
            "cannot write provider system secret; run with sudo or install via a privileged helper",
        ),
    }
}
