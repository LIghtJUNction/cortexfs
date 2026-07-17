use crate::*;

const OAUTH_CALLBACK_RESPONSE_BODY: &str =
    "CortexFS OAuth login complete. You may close this tab.\n";

/// Starts OAuth login flow for a provider and waits for callback completion.
pub(crate) fn provider_oauth_login(
    provider: &str,
    timeout_secs: u64,
    device: bool,
) -> Result<(), CliError> {
    let config = provider_oauth_config(provider)?;
    if device {
        return provider_oauth_device_login(provider, &config, timeout_secs);
    }
    let pkce = cortexfs::OAuthPkce::from_entropy(&read_system_entropy(32)?)
        .map_err(|_error| CliError::unavailable("cannot create oauth pkce verifier"))?;
    let state = hex_bytes(&read_system_entropy(16)?);
    let auth_url = cortexfs::oauth_authorization_url(&config, &state, &pkce)
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
        let _ignored = ProcessCommand::new("/usr/bin/xdg-open")
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

    let form = cortexfs::oauth_authorization_code_form(&config, &code, &pkce)
        .map_err(|_error| CliError::usage("invalid oauth authorization code exchange"))?;
    let token = cortexfs::exchange_oauth_token(&config, &form)
        .map_err(|_error| CliError::unavailable("oauth token exchange failed"))?;
    persist_oauth_tokens(provider, &config, &token)?;
    print_line("oauth login ok")
}

fn provider_oauth_device_login(
    provider: &str,
    config: &cortexfs::OAuthProviderConfig,
    timeout_secs: u64,
) -> Result<(), CliError> {
    let post = |url: &str, body: &str| cortexfs::oauth_post(url, "application/json", body, 30);
    let device = cortexfs::request_device_code_with(post)
        .map_err(|_error| CliError::unavailable("oauth device code request failed"))?;
    print_line(&format!(
        "open {} and enter code {}",
        cortexfs::CODEX_DEVICE_VERIFY_URL,
        device.code
    ))?;
    let token = cortexfs::poll_device_code_with(
        &device,
        timeout_secs,
        post,
        |url, body| cortexfs::oauth_post(url, "application/x-www-form-urlencoded", body, 30),
        |seconds| std::thread::sleep(Duration::from_secs(seconds)),
    )
    .map_err(|_error| CliError::unavailable("oauth device code login failed"))?;
    persist_oauth_tokens(provider, config, &token)?;
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
    for (label, slot) in [
        ("access_token", config.access_account()),
        ("refresh_token", config.refresh_account()),
        ("account_id", "oauth:account"),
        ("expires_at", "oauth:expires-at"),
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
    let config = provider_oauth_config(provider)?;
    let refresh = if oauth_uses_system_store(&config)? {
        cortexfs::read_codex_system()
            .map_err(|_error| CliError::unavailable("oauth credential store unavailable"))?
            .map(|state| state.refresh_token)
    } else {
        cortexfs::oauth_keychain_secret(
            &cortexfs::provider_keychain_service(provider),
            config.refresh_account(),
        )
        .map_err(|_error| CliError::unavailable("system secret store unavailable"))?
    }
    .ok_or_else(|| CliError::unavailable("oauth refresh token is not configured"))?;
    let form = cortexfs::oauth_refresh_token_form(&config, &refresh)
        .map_err(|_error| CliError::usage("invalid oauth refresh config"))?;
    let token = cortexfs::exchange_oauth_token(&config, &form)
        .map_err(|_error| CliError::unavailable("oauth token exchange failed"))?;
    persist_oauth_tokens(provider, &config, &token)?;
    print_line("oauth refresh ok")
}

pub(crate) fn provider_oauth_config(
    provider: &str,
) -> Result<cortexfs::OAuthProviderConfig, CliError> {
    if !is_provider_name(provider) {
        return Err(CliError::usage("invalid provider name"));
    }
    let config = read_provider_config_from_dir(provider, Path::new(PROVIDER_CONFIG_DIR))?
        .oauth
        .ok_or_else(|| CliError::usage(format!("provider has no oauth config: {provider}")))?;
    Ok(config)
}

fn persist_oauth_tokens(
    provider: &str,
    config: &cortexfs::OAuthProviderConfig,
    token: &cortexfs::OAuthTokenResponse,
) -> Result<(), CliError> {
    let now = current_time_unix();
    if oauth_uses_system_store(config)? {
        let retained = cortexfs::read_codex_system()
            .map_err(|_error| CliError::unavailable("oauth credential store unavailable"))?;
        let state = cortexfs::oauth_token_state(token, retained.as_ref(), now)
            .map_err(|_error| CliError::unavailable("oauth credential store unavailable"))?;
        return cortexfs::store_codex_system(&state)
            .map_err(|_error| CliError::unavailable("oauth credential store unavailable"));
    }
    cortexfs::store_oauth_tokens(provider, config, token, now)
        .map_err(|_error| CliError::unavailable("oauth credential store unavailable"))
}

fn oauth_uses_system_store(config: &cortexfs::OAuthProviderConfig) -> Result<bool, CliError> {
    Ok(config.is_codex() && current_uid_text().map_err(CliError::unavailable)? == "0")
}
