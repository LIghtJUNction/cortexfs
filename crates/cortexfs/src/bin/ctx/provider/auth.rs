use crate::*;

pub(super) mod socket;

/// Stores an API key as one complete named authentication profile.
pub(crate) fn provider_api_key_login(provider: &str, profile: &str) -> Result<(), CliError> {
    let config = read_provider_config_from_dir(provider, Path::new(PROVIDER_CONFIG_DIR))?;
    if !config
        .auth_methods()
        .iter()
        .any(|method| method.method == cortexfs::AuthMethod::ApiKey)
    {
        return Err(CliError::usage(
            "provider does not accept API-key authentication",
        ));
    }
    let key = read_provider_secret_stdin_limited(io::stdin(), MAX_PROVIDER_SECRET_STDIN_BYTES)
        .map_err(|_error| CliError::unavailable("cannot read API key from stdin"))?;
    let key = key.trim_end_matches(['\r', '\n']);
    if key.is_empty() {
        return Err(CliError::usage(
            "auth login reads a non-empty API key from stdin",
        ));
    }
    socket::api_key_login(provider, profile, key)?;
    print_line("auth login ok")
}

/// Prints the provider-neutral authentication methods from host config.
pub(crate) fn provider_auth_methods(provider: &str) -> Result<(), CliError> {
    let config = read_provider_config_from_dir(provider, Path::new(PROVIDER_CONFIG_DIR))?;
    if config.auth.iter().any(|method| !method.is_valid())
        || config
            .auth
            .iter()
            .any(|method| method.method == cortexfs::AuthMethod::OAuth && config.oauth.is_none())
    {
        return Err(CliError::usage("invalid provider auth config"));
    }
    for method in config.auth_methods() {
        let kind = match method.method {
            cortexfs::AuthMethod::ApiKey => "api_key",
            cortexfs::AuthMethod::OAuth => "oauth",
        };
        let flow = method.flow.map_or("none", |flow| match flow {
            cortexfs::OAuthFlow::AuthorizationCode => "authorization_code",
            cortexfs::OAuthFlow::DeviceCode => "device_code",
        });
        print_line(&format!("{kind}\t{flow}\t{}", method.slot))?;
    }
    Ok(())
}
