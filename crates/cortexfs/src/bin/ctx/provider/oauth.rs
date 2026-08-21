use crate::*;

/// Starts OAuth login flow for a provider and waits for callback completion.
pub(crate) fn provider_oauth_login(
    provider: &str,
    profile: &str,
    timeout_secs: u64,
    device: bool,
) -> Result<(), CliError> {
    let provider_config = provider_config(provider)?;
    let config = provider_config
        .oauth
        .clone()
        .ok_or_else(|| CliError::usage("provider has no oauth config"))?;
    let registry = cortexfs::configured_registry(
        provider,
        &provider_config.base_url,
        provider_config.auth_methods(),
        provider_config.oauth.clone(),
    )
    .ok_or_else(|| CliError::usage("provider auth adapter is unavailable"))?;
    let adapter = registry
        .get(provider)
        .ok_or_else(|| CliError::usage("provider auth adapter is unavailable"))?;
    if device {
        let _ = adapter;
        return auth::socket::oauth_device_login(
            provider,
            profile,
            &provider_config,
            config,
            timeout_secs,
        );
    }
    auth::socket::oauth_browser_login(provider, profile, &provider_config, config, timeout_secs)
}

pub(crate) fn provider_oauth_status(provider: &str, profile_name: &str) -> Result<(), CliError> {
    let config = provider_oauth_config(provider)?;
    if let Some(profile) = cortexfs::read_auth_profile(provider, profile_name)
        .map_err(|_error| CliError::unavailable("oauth credential store unavailable"))?
    {
        let &cortexfs::Credential::OAuth {
            ref refresh_token,
            expires_at,
            ..
        } = profile.credential()
        else {
            return Err(CliError::usage("authentication profile is not OAuth"));
        };
        for (label, present) in [
            ("access_token", true),
            ("refresh_token", refresh_token.is_some()),
            ("account_id", false),
            ("expires_at", expires_at.is_some()),
        ] {
            print_line(&format!(
                "oauth {label}={}",
                if present { "configured" } else { "missing" }
            ))?;
        }
        return Ok(());
    }
    let system = oauth_uses_system_store(&config)?;
    let stored = system
        .then(|| {
            cortexfs::read_codex_system()
                .map_err(|_error| CliError::unavailable("oauth credential store unavailable"))
        })
        .transpose()?
        .flatten()
        .is_some();
    let service = cortexfs::provider_keychain_service(provider);
    let expiry_slot = if config.is_codex() {
        "oauth:expires-at"
    } else {
        "oauth:expires"
    };
    for (label, slot) in [
        ("access_token", config.access_account()),
        ("refresh_token", config.refresh_account()),
        ("account_id", "oauth:account"),
        ("expires_at", expiry_slot),
    ] {
        let present = if system {
            stored
        } else {
            cortexfs::oauth_keychain_secret(&service, slot)
                .map_err(|_error| CliError::unavailable("system secret store unavailable"))?
                .is_some()
        };
        print_line(&format!(
            "oauth {label}={}",
            if present { "configured" } else { "missing" }
        ))?;
    }
    Ok(())
}

pub(crate) fn provider_oauth_refresh(provider: &str, profile: &str) -> Result<(), CliError> {
    let provider_config = provider_config(provider)?;
    let config = provider_config
        .oauth
        .clone()
        .ok_or_else(|| CliError::usage("provider has no oauth config"))?;
    let registry = cortexfs::configured_registry(
        provider,
        &provider_config.base_url,
        provider_config.auth_methods(),
        provider_config.oauth.clone(),
    )
    .ok_or_else(|| CliError::usage("provider auth adapter is unavailable"))?;
    let adapter = registry
        .get(provider)
        .ok_or_else(|| CliError::usage("provider auth adapter is unavailable"))?;
    let credential = stored_oauth_credential(provider, profile, &config)?;
    let mut transport = cortexfs::http_transport()
        .map_err(|_error| CliError::unavailable("oauth transport unavailable"))?;
    let refreshed = adapter
        .refresh_with(&credential, &mut transport, current_time_unix())
        .map_err(|_error| CliError::unavailable("oauth token exchange failed"))?;
    persist_adapter_credential(&config, adapter, profile, &refreshed)?;
    print_line("oauth refresh ok")
}

pub(crate) fn provider_auth_logout(provider: &str, profile: &str) -> Result<(), CliError> {
    cortexfs::delete_auth_profile(provider, profile)
        .map_err(|_error| CliError::unavailable("authentication credential store unavailable"))?;
    if let Ok(config) = provider_oauth_config(provider) {
        cortexfs::delete_oauth_credentials(provider, &config)
            .map_err(|_error| CliError::unavailable("OAuth credential store unavailable"))?;
    }
    print_line("provider authentication removed")
}

fn provider_config(provider: &str) -> Result<CtxProviderConfig, CliError> {
    if !is_provider_name(provider) {
        return Err(CliError::usage("invalid provider name"));
    }
    read_provider_config_from_dir(provider, Path::new(PROVIDER_CONFIG_DIR))
}

pub(crate) fn provider_oauth_config(
    provider: &str,
) -> Result<cortexfs::OAuthProviderConfig, CliError> {
    let config = provider_config(provider)?
        .oauth
        .ok_or_else(|| CliError::usage(format!("provider has no oauth config: {provider}")))?;
    Ok(config)
}

fn stored_oauth_credential(
    provider: &str,
    profile_name: &str,
    config: &cortexfs::OAuthProviderConfig,
) -> Result<cortexfs::Credential, CliError> {
    if let Some(profile) = cortexfs::read_auth_profile(provider, profile_name)
        .map_err(|_error| CliError::unavailable("oauth credential store unavailable"))?
    {
        return match *profile.credential() {
            cortexfs::Credential::OAuth { .. } => Ok(profile.credential().clone()),
            cortexfs::Credential::ApiKey { .. } => {
                Err(CliError::usage("authentication profile is not OAuth"))
            }
        };
    }
    if oauth_uses_system_store(config)? {
        let state = cortexfs::read_codex_system()
            .map_err(|_error| CliError::unavailable("oauth credential store unavailable"))?
            .ok_or_else(|| CliError::unavailable("oauth credential is not configured"))?;
        return Ok(cortexfs::Credential::OAuth {
            provider: provider.to_owned(),
            access_token: state.access_token,
            refresh_token: Some(state.refresh_token),
            expires_at: Some(state.expires_at),
            scopes: Vec::new(),
        });
    }
    let service = cortexfs::provider_keychain_service(provider);
    let access = cortexfs::oauth_keychain_secret(&service, config.access_account())
        .map_err(|_error| CliError::unavailable("system secret store unavailable"))?
        .ok_or_else(|| CliError::unavailable("oauth access token is not configured"))?;
    let refresh = cortexfs::oauth_keychain_secret(&service, config.refresh_account())
        .map_err(|_error| CliError::unavailable("system secret store unavailable"))?;
    let expiry_slot = if config.is_codex() {
        "oauth:expires-at"
    } else {
        "oauth:expires"
    };
    let expires_at = cortexfs::oauth_keychain_secret(&service, expiry_slot)
        .map_err(|_error| CliError::unavailable("system secret store unavailable"))?
        .and_then(|value| value.parse().ok());
    Ok(cortexfs::Credential::OAuth {
        provider: provider.to_owned(),
        access_token: access,
        refresh_token: refresh,
        expires_at,
        scopes: Vec::new(),
    })
}

fn persist_adapter_credential(
    config: &cortexfs::OAuthProviderConfig,
    adapter: &dyn cortexfs::AuthProvider,
    profile: &str,
    credential: &cortexfs::Credential,
) -> Result<(), CliError> {
    let _ = config;
    cortexfs::store_auth_profile(adapter.id(), profile, credential.clone())
        .map(|_profile| ())
        .map_err(|_error| CliError::unavailable("oauth credential store unavailable"))
}

fn oauth_uses_system_store(config: &cortexfs::OAuthProviderConfig) -> Result<bool, CliError> {
    Ok(config.is_codex() && current_uid_text().map_err(CliError::unavailable)? == "0")
}
