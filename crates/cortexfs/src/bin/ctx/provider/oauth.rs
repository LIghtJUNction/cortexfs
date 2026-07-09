use crate::*;

pub(crate) const CTX_PROVIDER_CURL_BIN: &str = "/usr/bin/curl";
const OAUTH_CALLBACK_RESPONSE_BODY: &str =
    "CortexFS OAuth login complete. You may close this tab.\n";
const MAX_OAUTH_TOKEN_RESPONSE_BYTES: u64 = 1024 * 1024;

pub(crate) fn provider_oauth_login(provider: &str, timeout_secs: u64) -> Result<(), CliError> {
    let config = provider_oauth_config(provider)?;
    let pkce = oauth_pkce_from_system_entropy()?;
    let state = oauth_state_from_system_entropy()?;
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
    print_line(&format!(
        "waiting for OAuth callback on {} for {}s",
        config.redirect_uri, timeout_secs
    ))?;

    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
    let mut stream = accept_oauth_callback(&listener, deadline)?;
    let request = read_oauth_callback_request(&mut stream, deadline)?;
    let params = parse_oauth_callback_params(&request, &callback.path)?;
    if params.state.as_deref() != Some(state.as_str()) {
        return Err(CliError::usage("oauth callback state mismatch"));
    }
    let code = params
        .code
        .ok_or_else(|| CliError::usage("oauth callback missing code"))?;
    let response = oauth_callback_response();
    stream
        .write_all(response.as_bytes())
        .map_err(|error| CliError::unavailable(format!("cannot write oauth callback: {error}")))?;

    let form = cortexfs::oauth_authorization_code_form(&config, &code, &pkce)
        .map_err(|_error| CliError::usage("invalid oauth authorization code exchange"))?;
    let token = exchange_oauth_token(&config.token_url, &form)?;
    store_oauth_tokens(provider, &config, &token)?;
    print_line("oauth login ok")
}

pub(crate) fn oauth_callback_response() -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: {}\r\n\r\n{}",
        OAUTH_CALLBACK_RESPONSE_BODY.len(),
        OAUTH_CALLBACK_RESPONSE_BODY
    )
}

pub(crate) fn provider_oauth_status(provider: &str) -> Result<(), CliError> {
    let config = provider_oauth_config(provider)?;
    let service = provider_keychain_service(provider);
    let access = keychain_has_secret(&service, config.access_account())?;
    let refresh = keychain_has_secret(&service, config.refresh_account())?;
    print_line(&format!(
        "oauth access_token={}",
        if access { "configured" } else { "missing" }
    ))?;
    print_line(&format!(
        "oauth refresh_token={}",
        if refresh { "configured" } else { "missing" }
    ))
}

pub(crate) fn accept_oauth_callback(
    listener: &std::net::TcpListener,
    deadline: std::time::Instant,
) -> Result<std::net::TcpStream, CliError> {
    loop {
        match listener.accept() {
            Ok((stream, _addr)) => return Ok(stream),
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
    }
}

pub(crate) fn provider_oauth_refresh(provider: &str) -> Result<(), CliError> {
    let config = provider_oauth_config(provider)?;
    let service = provider_keychain_service(provider);
    let refresh = keychain_get_secret(&service, config.refresh_account())?
        .ok_or_else(|| CliError::unavailable("oauth refresh token is not configured"))?;
    let form = cortexfs::oauth_refresh_token_form(&config, &refresh)
        .map_err(|_error| CliError::usage("invalid oauth refresh config"))?;
    let token = exchange_oauth_token(&config.token_url, &form)?;
    store_oauth_tokens(provider, &config, &token)?;
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

pub(crate) fn exchange_oauth_token(
    token_url: &str,
    form: &str,
) -> Result<cortexfs::OAuthTokenResponse, CliError> {
    let mut child = ctx_provider_curl_command()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| CliError::unavailable(format!("cannot start curl: {error}")))?;
    let Some(mut stdin) = child.stdin.take() else {
        terminate_process_child(&mut child);
        return Err(CliError::unavailable("cannot write curl config"));
    };
    let config = format!(
        "fail\nsilent\nshow-error\nmax-time = 30\nrequest = POST\nurl = {}\nheader = {}\ndata = {}\n",
        curl_config_quote(token_url)?,
        curl_config_quote("Content-Type: application/x-www-form-urlencoded")?,
        curl_config_quote(form)?,
    );
    stdin
        .write_all(config.as_bytes())
        .map_err(|error| CliError::unavailable(format!("cannot write curl config: {error}")))?;
    drop(stdin);
    let Some(stdout) = child.stdout.take() else {
        terminate_process_child(&mut child);
        return Err(CliError::unavailable("cannot read curl output"));
    };
    let mut limited = stdout.take(MAX_OAUTH_TOKEN_RESPONSE_BYTES + 1);
    let mut output = Vec::new();
    limited
        .read_to_end(&mut output)
        .map_err(|error| CliError::unavailable(format!("cannot read token response: {error}")))?;
    if u64::try_from(output.len()).unwrap_or(u64::MAX) > MAX_OAUTH_TOKEN_RESPONSE_BYTES {
        terminate_process_child(&mut child);
        return Err(CliError::unavailable("oauth token response too large"));
    }
    let status = child
        .wait()
        .map_err(|error| CliError::unavailable(format!("cannot run curl: {error}")))?;
    if !status.success() {
        return Err(CliError::unavailable("oauth token exchange failed"));
    }
    cortexfs::parse_oauth_token_response(&output)
        .map_err(|_error| CliError::unavailable("invalid oauth token response"))
}

pub(crate) fn ctx_provider_curl_command() -> ProcessCommand {
    let mut command = ProcessCommand::new(CTX_PROVIDER_CURL_BIN);
    command.env_clear().arg("-q").arg("--config").arg("-");
    command
}

pub(crate) fn store_oauth_tokens(
    provider: &str,
    config: &cortexfs::OAuthProviderConfig,
    token: &cortexfs::OAuthTokenResponse,
) -> Result<(), CliError> {
    let service = provider_keychain_service(provider);
    keychain_set_secret(&service, config.access_account(), &token.access_token)?;
    if let Some(refresh) = token.refresh_token.as_deref()
        && !refresh.trim().is_empty()
    {
        keychain_set_secret(&service, config.refresh_account(), refresh)?;
    }
    Ok(())
}

pub(crate) fn keychain_has_secret(service: &str, account: &str) -> Result<bool, CliError> {
    keychain_get_secret(service, account).map(|value| value.is_some())
}

pub(crate) fn keychain_get_secret(
    service: &str,
    account: &str,
) -> Result<Option<String>, CliError> {
    let entry = match keyring::Entry::new(service, account) {
        Ok(entry) => entry,
        Err(keyring::Error::NoDefaultStore) => return Ok(None),
        Err(_error) => return Err(CliError::unavailable("system keychain unavailable")),
    };
    match entry.get_password() {
        Ok(secret) if secret.is_empty() => Ok(None),
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_error) => Err(CliError::unavailable("system keychain unavailable")),
    }
}

pub(crate) fn keychain_set_secret(
    service: &str,
    account: &str,
    secret: &str,
) -> Result<(), CliError> {
    let entry = keyring::Entry::new(service, account)
        .map_err(|_error| CliError::unavailable("system keychain unavailable"))?;
    entry
        .set_password(secret)
        .map_err(|_error| CliError::unavailable("system keychain unavailable"))
}

pub(crate) fn provider_keychain_service(provider: &str) -> String {
    format!("cortexfs:{provider}")
}

pub(crate) fn oauth_pkce_from_system_entropy() -> Result<cortexfs::OAuthPkce, CliError> {
    let entropy = read_system_entropy(32)?;
    cortexfs::OAuthPkce::from_entropy(&entropy)
        .map_err(|_error| CliError::unavailable("cannot create oauth pkce verifier"))
}

pub(crate) fn oauth_state_from_system_entropy() -> Result<String, CliError> {
    Ok(hex_bytes(&read_system_entropy(16)?))
}

pub(crate) fn read_system_entropy(size: usize) -> Result<Vec<u8>, CliError> {
    let mut file = fs::File::open("/dev/urandom")
        .map_err(|error| CliError::unavailable(format!("cannot read system entropy: {error}")))?;
    let mut bytes = vec![0; size];
    file.read_exact(&mut bytes)
        .map_err(|error| CliError::unavailable(format!("cannot read system entropy: {error}")))?;
    Ok(bytes)
}

pub(crate) fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::new();
    for byte in bytes {
        let _ignored = write!(output, "{byte:02x}");
    }
    output
}
