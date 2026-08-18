use crate::*;

const OAUTH_CALLBACK_RESPONSE_BODY: &str =
    "CortexFS OAuth login complete. You may close this tab.\n";

/// Starts OAuth login flow for a provider and waits for callback completion.
pub(crate) fn provider_oauth_login(
    provider: &str,
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
        return provider_oauth_device_login(&config, adapter, timeout_secs);
    }
    let pkce = cortexfs::OAuthPkce::from_entropy(&read_system_entropy(32)?)
        .map_err(|_error| CliError::unavailable("cannot create oauth pkce verifier"))?;
    let state = hex_bytes(&read_system_entropy(16)?);
    let auth_url = adapter
        .authorization_url(&state, &pkce)
        .map_err(|_error| CliError::usage("invalid provider oauth config"))?;
    let callback = parse_oauth_redirect_uri(&config.redirect_uri)?;
    let listener =
        std::net::TcpListener::bind((callback.host.as_str(), callback.port)).map_err(|error| {
            CliError::unavailable(format!("cannot listen on oauth redirect uri: {error}"))
        })?;
    listener.set_nonblocking(true).map_err(|error| {
        CliError::unavailable(format!("cannot configure oauth listener: {error}"))
    })?;

    print_line("open this URL in your browser:")?;
    print_line(&auth_url)?;
    if ["DISPLAY", "WAYLAND_DISPLAY"]
        .iter()
        .any(|name| env::var_os(name).is_some_and(|value| !value.is_empty()))
    {
        let _ignored = ProcessCommand::new(cortexfs::support::command::XDG_OPEN)
            .arg(&auth_url)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
    print_line(&format!(
        "waiting for OAuth callback on {} for {}s",
        config.redirect_uri, timeout_secs
    ))?;

    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    let mut stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if std::time::Instant::now() >= deadline {
                    return Err(CliError::unavailable("oauth callback timed out"));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "oauth callback failed: {error}"
                )));
            }
        }
    };
    let request = read_oauth_callback_request(&mut stream, deadline)?;
    let params = parse_oauth_callback_params(&request, &callback.path)?;
    if params.state.as_deref() != Some(state.as_str()) {
        return Err(CliError::usage("oauth callback state mismatch"));
    }
    let code = params
        .code
        .ok_or_else(|| CliError::usage("oauth callback missing code"))?;
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: {}\r\n\r\n{}",
        OAUTH_CALLBACK_RESPONSE_BODY.len(),
        OAUTH_CALLBACK_RESPONSE_BODY
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|error| CliError::unavailable(format!("cannot write oauth callback: {error}")))?;

    let mut transport = cortexfs::http_transport()
        .map_err(|_error| CliError::unavailable("oauth transport unavailable"))?;
    let credential = adapter
        .login_with(
            cortexfs::AuthRequest::AuthorizationCodePkce {
                code,
                verifier: pkce.verifier().to_owned(),
            },
            &mut transport,
            current_time_unix(),
        )
        .map_err(|_error| CliError::unavailable("oauth token exchange failed"))?;
    persist_adapter_credential(&config, adapter, &credential)?;
    print_line("oauth login ok")
}

fn provider_oauth_device_login(
    config: &cortexfs::OAuthProviderConfig,
    adapter: &dyn cortexfs::AuthProvider,
    timeout_secs: u64,
) -> Result<(), CliError> {
    let mut transport = cortexfs::http_transport()
        .map_err(|_error| CliError::unavailable("oauth transport unavailable"))?;
    let credential = adapter
        .device_login_with(
            timeout_secs,
            &mut transport,
            current_time_unix(),
            &mut |challenge| {
                let _ignored = print_line(&format!(
                    "open {} and enter code {}",
                    challenge.verification_uri, challenge.user_code
                ));
            },
            &mut |seconds| std::thread::sleep(Duration::from_secs(seconds)),
        )
        .map_err(|_error| CliError::unavailable("oauth device code login failed"))?;
    persist_adapter_credential(config, adapter, &credential)?;
    print_line("oauth login ok")
}

pub(crate) fn provider_oauth_status(provider: &str) -> Result<(), CliError> {
    let config = provider_oauth_config(provider)?;
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

pub(crate) fn provider_oauth_refresh(provider: &str) -> Result<(), CliError> {
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
    let credential = stored_oauth_credential(provider, &config)?;
    let mut transport = cortexfs::http_transport()
        .map_err(|_error| CliError::unavailable("oauth transport unavailable"))?;
    let refreshed = adapter
        .refresh_with(&credential, &mut transport, current_time_unix())
        .map_err(|_error| CliError::unavailable("oauth token exchange failed"))?;
    persist_adapter_credential(&config, adapter, &refreshed)?;
    print_line("oauth refresh ok")
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
    config: &cortexfs::OAuthProviderConfig,
) -> Result<cortexfs::Credential, CliError> {
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
    credential: &cortexfs::Credential,
) -> Result<(), CliError> {
    if oauth_uses_system_store(config)? {
        let &cortexfs::Credential::OAuth {
            ref access_token,
            ref refresh_token,
            expires_at,
            ref scopes,
            ..
        } = credential
        else {
            return Err(CliError::usage(
                "oauth login returned an invalid credential",
            ));
        };
        let token = cortexfs::OAuthTokenResponse {
            access_token: access_token.clone(),
            token_type: Some("Bearer".to_owned()),
            expires_in: expires_at.and_then(|value| value.checked_sub(current_time_unix())),
            refresh_token: refresh_token.clone(),
            scope: (!scopes.is_empty()).then(|| scopes.join(" ")),
            id_token: None,
        };
        let retained = cortexfs::read_codex_system()
            .map_err(|_error| CliError::unavailable("oauth credential store unavailable"))?;
        let state = cortexfs::oauth_token_state(&token, retained.as_ref(), current_time_unix())
            .map_err(|_error| CliError::unavailable("oauth credential store unavailable"))?;
        return cortexfs::store_codex_system(&state)
            .map_err(|_error| CliError::unavailable("oauth credential store unavailable"));
    }
    adapter
        .persist(credential, current_time_unix())
        .map_err(|_error| CliError::unavailable("oauth credential store unavailable"))
}

fn oauth_uses_system_store(config: &cortexfs::OAuthProviderConfig) -> Result<bool, CliError> {
    Ok(config.is_codex() && current_uid_text().map_err(CliError::unavailable)? == "0")
}
