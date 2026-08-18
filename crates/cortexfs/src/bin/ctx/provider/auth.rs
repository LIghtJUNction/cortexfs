use crate::*;

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
