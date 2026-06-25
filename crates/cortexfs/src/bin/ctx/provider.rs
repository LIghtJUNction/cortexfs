const PROVIDER_CONFIG_DIR: &str = "/etc/cortexfs/providers.d";
const OAUTH_CALLBACK_RESPONSE_BODY: &str =
    "CortexFS OAuth login complete. You may close this tab.\n";
const MAX_OAUTH_TOKEN_RESPONSE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
struct CtxProviderConfig {
    base_url: String,
    oauth: Option<cortexfs::OAuthProviderConfig>,
}

fn provider_command(args: &ProviderArgs) -> Result<ExitCode, CliError> {
    match *args {
        ProviderArgs::Login {
            ref provider,
            timeout,
        } => {
            provider_oauth_login(provider, timeout).map(|()| ExitCode::SUCCESS)
        }
        ProviderArgs::Status { ref provider } => {
            provider_oauth_status(provider).map(|()| ExitCode::SUCCESS)
        }
        ProviderArgs::Refresh { ref provider } => {
            provider_oauth_refresh(provider).map(|()| ExitCode::SUCCESS)
        }
        ProviderArgs::PresetList => provider_preset_list().map(|()| ExitCode::SUCCESS),
        ProviderArgs::PresetShow { ref preset } => {
            provider_preset_show(preset).map(|()| ExitCode::SUCCESS)
        }
        ProviderArgs::PresetInstall { ref preset } => {
            provider_preset_install(preset).map(|()| ExitCode::SUCCESS)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProviderPreset {
    name: &'static str,
    aliases: &'static [&'static str],
    file: &'static str,
    config: &'static str,
}

const PROVIDER_PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        name: "openai",
        aliases: &["codex", "ccodex"],
        file: "api.openai.com.json",
        config: r#"{
  "base_url": "https://api.openai.com/v1",
  "api_key_env": "OPENAI_API_KEY",
  "enabled": true,
  "formats": ["openai.chat", "openai.responses"]
}
"#,
    },
    ProviderPreset {
        name: "anthropic",
        aliases: &["claude"],
        file: "api.anthropic.com.json",
        config: r#"{
  "base_url": "https://api.anthropic.com/v1",
  "api_key_env": "ANTHROPIC_API_KEY",
  "enabled": true,
  "formats": ["anthropic.messages"]
}
"#,
    },
    ProviderPreset {
        name: "google",
        aliases: &["gemini"],
        file: "generativelanguage.googleapis.com.json",
        config: r#"{
  "base_url": "https://generativelanguage.googleapis.com/v1beta/openai/",
  "api_key_env": "GOOGLE_API_KEY",
  "enabled": true,
  "formats": ["openai.chat"]
}
"#,
    },
];

fn provider_preset_list() -> Result<(), CliError> {
    for preset in PROVIDER_PRESETS {
        print_line(preset.name)?;
    }
    Ok(())
}

fn provider_preset_show(preset: &str) -> Result<(), CliError> {
    let preset = provider_preset(preset)?;
    print_line(preset.config.trim_end())
}

fn provider_preset_install(preset: &str) -> Result<(), CliError> {
    let preset = provider_preset(preset)?;
    fs::create_dir_all(PROVIDER_CONFIG_DIR)
        .map_err(|error| CliError::unavailable(format!("cannot create provider config dir: {error}")))?;
    let path = PathBuf::from(PROVIDER_CONFIG_DIR).join(preset.file);
    atomic_write_provider_config(&path, preset.config)?;
    print_line(&format!("installed {}", path.display()))
}

fn provider_preset(name: &str) -> Result<ProviderPreset, CliError> {
    PROVIDER_PRESETS
        .iter()
        .copied()
        .find(|preset| preset.name == name || preset.aliases.contains(&name))
        .ok_or_else(|| CliError::usage(format!("unknown provider preset: {name}")))
}

fn atomic_write_provider_config(path: &Path, content: &str) -> Result<(), CliError> {
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, content)
        .map_err(|error| CliError::unavailable(format!("cannot write provider config: {error}")))?;
    fs::rename(&temp, path)
        .map_err(|error| CliError::unavailable(format!("cannot install provider config: {error}")))
}

fn provider_oauth_login(provider: &str, timeout_secs: u64) -> Result<(), CliError> {
    let config = provider_oauth_config(provider)?;
    let pkce = oauth_pkce_from_system_entropy()?;
    let state = oauth_state_from_system_entropy()?;
    let auth_url = cortexfs::oauth_authorization_url(&config, &state, &pkce)
        .map_err(|_error| CliError::usage("invalid provider oauth config"))?;
    let callback = parse_oauth_redirect_uri(&config.redirect_uri)?;
    let listener = std::net::TcpListener::bind((callback.host.as_str(), callback.port)).map_err(
        |error| CliError::unavailable(format!("cannot listen on oauth redirect uri: {error}")),
    )?;
    listener
        .set_nonblocking(true)
        .map_err(|error| CliError::unavailable(format!("cannot configure oauth listener: {error}")))?;

    print_line("open this URL in your browser:")?;
    print_line(&auth_url)?;
    print_line(&format!(
        "waiting for OAuth callback on {} for {}s",
        config.redirect_uri, timeout_secs
    ))?;

    let mut stream = accept_oauth_callback(&listener, timeout_secs)?;
    let request = read_oauth_callback_request(&mut stream)?;
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

fn oauth_callback_response() -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/plain; charset=utf-8\r\ncontent-length: {}\r\n\r\n{}",
        OAUTH_CALLBACK_RESPONSE_BODY.len(),
        OAUTH_CALLBACK_RESPONSE_BODY
    )
}

fn provider_oauth_status(provider: &str) -> Result<(), CliError> {
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

fn accept_oauth_callback(
    listener: &std::net::TcpListener,
    timeout_secs: u64,
) -> Result<std::net::TcpStream, CliError> {
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);
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
                return Err(CliError::unavailable(format!("oauth callback failed: {error}")));
            }
        }
    }
}

fn provider_oauth_refresh(provider: &str) -> Result<(), CliError> {
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

fn provider_oauth_config(provider: &str) -> Result<cortexfs::OAuthProviderConfig, CliError> {
    if !is_provider_name(provider) {
        return Err(CliError::usage("invalid provider name"));
    }
    let config = read_provider_config(provider)?
        .oauth
        .ok_or_else(|| CliError::usage(format!("provider has no oauth config: {provider}")))?;
    Ok(config)
}

fn read_provider_config(provider: &str) -> Result<CtxProviderConfig, CliError> {
    let entries = fs::read_dir(PROVIDER_CONFIG_DIR).map_err(|error| {
        CliError::unavailable(format!("cannot read provider config dir: {error}"))
    })?;
    for entry in entries {
        let entry = entry
            .map_err(|error| CliError::unavailable(format!("cannot read provider config: {error}")))?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let content = read_file_to_string(&path)?;
        let config = serde_json::from_str::<CtxProviderConfig>(&content)
            .map_err(|error| CliError::usage(format!("invalid provider config: {error}")))?;
        if provider_name_from_base_url(&config.base_url).as_deref() == Some(provider) {
            return Ok(config);
        }
    }
    Err(CliError::usage(format!("missing provider: {provider}")))
}

fn exchange_oauth_token(token_url: &str, form: &str) -> Result<cortexfs::OAuthTokenResponse, CliError> {
    let mut child = ProcessCommand::new("curl")
        .arg("--config")
        .arg("-")
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
        curl_config_quote(token_url),
        curl_config_quote("Content-Type: application/x-www-form-urlencoded"),
        curl_config_quote(form),
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

fn store_oauth_tokens(
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

fn keychain_has_secret(service: &str, account: &str) -> Result<bool, CliError> {
    keychain_get_secret(service, account).map(|value| value.is_some())
}

fn keychain_get_secret(service: &str, account: &str) -> Result<Option<String>, CliError> {
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

fn keychain_set_secret(service: &str, account: &str, secret: &str) -> Result<(), CliError> {
    let entry = keyring::Entry::new(service, account)
        .map_err(|_error| CliError::unavailable("system keychain unavailable"))?;
    entry
        .set_password(secret)
        .map_err(|_error| CliError::unavailable("system keychain unavailable"))
}

fn provider_keychain_service(provider: &str) -> String {
    format!("cortexfs:{provider}")
}

fn oauth_pkce_from_system_entropy() -> Result<cortexfs::OAuthPkce, CliError> {
    let entropy = read_system_entropy(32)?;
    cortexfs::OAuthPkce::from_entropy(&entropy)
        .map_err(|_error| CliError::unavailable("cannot create oauth pkce verifier"))
}

fn oauth_state_from_system_entropy() -> Result<String, CliError> {
    Ok(hex_bytes(&read_system_entropy(16)?))
}

fn read_system_entropy(size: usize) -> Result<Vec<u8>, CliError> {
    let mut file = fs::File::open("/dev/urandom")
        .map_err(|error| CliError::unavailable(format!("cannot read system entropy: {error}")))?;
    let mut bytes = vec![0; size];
    file.read_exact(&mut bytes)
        .map_err(|error| CliError::unavailable(format!("cannot read system entropy: {error}")))?;
    Ok(bytes)
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::new();
    for byte in bytes {
        let _ignored = write!(output, "{byte:02x}");
    }
    output
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OAuthRedirect {
    host: String,
    port: u16,
    path: String,
}

fn parse_oauth_redirect_uri(value: &str) -> Result<OAuthRedirect, CliError> {
    let rest = value
        .strip_prefix("http://")
        .ok_or_else(|| CliError::usage("oauth redirect_uri must use http:// localhost"))?;
    let (authority, path) = rest
        .split_once('/')
        .map_or_else(|| (rest, "/".to_owned()), |(authority, path)| {
            (authority, format!("/{path}"))
        });
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or_else(|| CliError::usage("oauth redirect_uri must include a port"))?;
    if !matches!(host, "127.0.0.1" | "localhost") {
        return Err(CliError::usage(
            "oauth redirect_uri must bind localhost or 127.0.0.1",
        ));
    }
    let port = port
        .parse::<u16>()
        .map_err(|_error| CliError::usage("oauth redirect_uri has invalid port"))?;
    Ok(OAuthRedirect {
        host: host.to_owned(),
        port,
        path,
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct OAuthCallbackParams {
    code: Option<String>,
    state: Option<String>,
}

fn read_oauth_callback_request(stream: &mut std::net::TcpStream) -> Result<String, CliError> {
    let mut buffer = [0_u8; 8192];
    let size = stream
        .read(&mut buffer)
        .map_err(|error| CliError::unavailable(format!("cannot read oauth callback: {error}")))?;
    let Some(bytes) = buffer.get(..size) else {
        return Err(CliError::unavailable("oauth callback exceeded buffer"));
    };
    String::from_utf8(bytes.to_vec())
        .map_err(|_error| CliError::usage("oauth callback must be valid UTF-8"))
}

fn parse_oauth_callback_params(
    request: &str,
    expected_path: &str,
) -> Result<OAuthCallbackParams, CliError> {
    let Some(first_line) = request.lines().next() else {
        return Err(CliError::usage("empty oauth callback"));
    };
    let mut fields = first_line.split_whitespace();
    let method = fields.next().unwrap_or_default();
    let target = fields.next().unwrap_or_default();
    if method != "GET" {
        return Err(CliError::usage("oauth callback must use GET"));
    }
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    if path != expected_path {
        return Err(CliError::usage("oauth callback path mismatch"));
    }
    let mut code = None;
    let mut state = None;
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = percent_decode(value)?;
        match key {
            "code" => code = Some(value),
            "state" => state = Some(value),
            _ => {}
        }
    }
    Ok(OAuthCallbackParams { code, state })
}

fn percent_decode(value: &str) -> Result<String, CliError> {
    let bytes = value.as_bytes();
    let mut output = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let Some(&byte) = bytes.get(index) else {
            break;
        };
        match byte {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err(CliError::usage("invalid oauth callback encoding"));
                }
                let Some(&high_raw) = bytes.get(index + 1) else {
                    return Err(CliError::usage("invalid oauth callback encoding"));
                };
                let Some(&low_raw) = bytes.get(index + 2) else {
                    return Err(CliError::usage("invalid oauth callback encoding"));
                };
                let high = hex_value(high_raw)?;
                let low = hex_value(low_raw)?;
                output.push((high << 4) | low);
                index += 3;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(output).map_err(|_error| CliError::usage("invalid oauth callback encoding"))
}

fn hex_value(byte: u8) -> Result<u8, CliError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(CliError::usage("invalid oauth callback encoding")),
    }
}

fn provider_name_from_base_url(base_url: &str) -> Option<String> {
    let host = base_url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split(['/', ':'])
        .next()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

fn is_provider_name(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('/')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn curl_config_quote(value: &str) -> String {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        if matches!(character, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

fn terminate_process_child(child: &mut std::process::Child) {
    match child.kill() {
        Ok(()) | Err(_) => {}
    }
    match child.wait() {
        Ok(_) | Err(_) => {}
    }
}
