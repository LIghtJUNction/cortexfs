const PROVIDER_CONFIG_DIR: &str = "/etc/cortexfs/providers.d";
const MAX_CTX_PROVIDER_CONFIG_BYTES: u64 = 64 * 1024;
const MAX_PROVIDER_SECRET_STDIN_BYTES: usize = 8 * 1024;
const CTX_PROVIDER_CURL_BIN: &str = "/usr/bin/curl";
const OAUTH_CALLBACK_RESPONSE_BODY: &str =
    "CortexFS OAuth login complete. You may close this tab.\n";
const MAX_OAUTH_TOKEN_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_OAUTH_CALLBACK_REQUEST_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, Deserialize)]
struct CtxProviderConfig {
    name: Option<String>,
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
        ProviderArgs::SecretSet {
            ref provider,
            ref slot,
        } => provider_secret_set(provider, slot).map(|()| ExitCode::SUCCESS),
        ProviderArgs::SecretStatus {
            ref provider,
            ref slot,
        } => provider_secret_status(provider, slot).map(|()| ExitCode::SUCCESS),
        ProviderArgs::PresetList => provider_preset_list().map(|()| ExitCode::SUCCESS),
        ProviderArgs::PresetShow { ref preset } => {
            provider_preset_show(preset).map(|()| ExitCode::SUCCESS)
        }
        ProviderArgs::PresetInstall { ref preset } => {
            provider_preset_install(preset).map(|()| ExitCode::SUCCESS)
        }
    }
}

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
  "default_model": "gpt-5.5",
  "models": ["gpt-5.4-mini"],
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
    create_provider_config_dir(Path::new(PROVIDER_CONFIG_DIR))?;
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
    let parent = path
        .parent()
        .ok_or_else(|| CliError::unavailable("provider config path has no parent"))?;
    let parent_dir = open_provider_config_dir(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::unavailable("provider config path has no file name"))?;
    for attempt in 0..16 {
        let temp_name = temp_file_name(attempt);
        let file_fd = match nix::fcntl::openat(
            &parent_dir,
            temp_name.as_str(),
            nix::fcntl::OFlag::O_CREAT
                | nix::fcntl::OFlag::O_EXCL
                | nix::fcntl::OFlag::O_WRONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::from_bits_truncate(0o600),
        ) {
            Ok(file_fd) => file_fd,
            Err(nix::errno::Errno::EEXIST) => continue,
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot write provider config: {error}"
                )));
            }
        };
        let mut temp = fs::File::from(file_fd);
        temp.write_all(content.as_bytes())
            .and_then(|()| temp.flush())
            .and_then(|()| temp.sync_all())
            .map_err(|error| {
                let _ignored = nix::unistd::unlinkat(
                    &parent_dir,
                    temp_name.as_str(),
                    nix::unistd::UnlinkatFlags::NoRemoveDir,
                );
                CliError::unavailable(format!("cannot write provider config: {error}"))
            })?;
        drop(temp);
        nix::fcntl::renameat(&parent_dir, temp_name.as_str(), &parent_dir, file_name).map_err(
            |error| {
                let _ignored = nix::unistd::unlinkat(
                    &parent_dir,
                    temp_name.as_str(),
                    nix::unistd::UnlinkatFlags::NoRemoveDir,
                );
                CliError::unavailable(format!("cannot install provider config: {error}"))
            },
        )?;
        return parent_dir.sync_all().map_err(|error| {
            CliError::unavailable(format!("cannot sync provider config dir: {error}"))
        });
    }
    Err(CliError::unavailable(
        "cannot create unique provider config temp file",
    ))
}

fn create_provider_config_dir(path: &Path) -> Result<(), CliError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            sync_provider_config_dir(path)
        } else {
            Err(CliError::unavailable(
                "provider config directory is not a plain directory",
            ))
        };
    }

    let mut missing = Vec::new();
    let mut cursor = Some(path);
    while let Some(current) = cursor {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
                return Err(CliError::unavailable(
                    "provider config path contains a non-directory entry",
                ));
            }
            Ok(_metadata) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                cursor = current.parent();
            }
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot inspect provider config dir: {error}"
                )));
            }
        }
    }

    let existing_parent = missing
        .last()
        .and_then(|path| path.parent())
        .ok_or_else(|| CliError::unavailable("invalid provider config dir"))?;
    let mut parent_dir = open_provider_config_dir(existing_parent)?;
    for directory in missing.iter().rev() {
        let name = provider_config_file_name(directory)?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(0o755),
        )
        .map_err(|error| {
            CliError::unavailable(format!("cannot create provider config dir: {error}"))
        })?;
        parent_dir.sync_all().map_err(|error| {
            CliError::unavailable(format!("cannot sync provider config dir: {error}"))
        })?;
        let child = nix::fcntl::openat(
            &parent_dir,
            name,
            nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(|error| {
            CliError::unavailable(format!("cannot open provider config dir: {error}"))
        })?;
        parent_dir = fs::File::from(child);
        parent_dir.sync_all().map_err(|error| {
            CliError::unavailable(format!("cannot sync provider config dir: {error}"))
        })?;
    }
    Ok(())
}

fn sync_provider_config_dir(path: &Path) -> Result<(), CliError> {
    let directory = open_provider_config_dir(path)?;
    directory
        .sync_all()
        .map_err(|error| CliError::unavailable(format!("cannot sync provider config dir: {error}")))
}

fn open_provider_config_dir(path: &Path) -> Result<fs::File, CliError> {
    let mut directory = if path.is_absolute() {
        open_single_provider_config_dir(Path::new("/"))?
    } else {
        open_single_provider_config_dir(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    CliError::unavailable(format!(
                        "cannot open provider config dir {}: invalid directory name",
                        path.display()
                    ))
                })?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )
                .map_err(|error| {
                    CliError::unavailable(format!(
                        "cannot open provider config dir {}: {error}",
                        path.display()
                    ))
                })?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(CliError::unavailable(format!(
                    "cannot open provider config dir {}: unsupported path component",
                    path.display()
                )));
            }
        }
    }
    Ok(directory)
}

fn open_single_provider_config_dir(path: &Path) -> Result<fs::File, CliError> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| {
            CliError::unavailable(format!("cannot open provider config dir: {error}"))
        })?;
    if !directory
        .metadata()
        .map_err(|error| {
            CliError::unavailable(format!("cannot inspect provider config dir: {error}"))
        })?
        .is_dir()
    {
        return Err(CliError::unavailable(
            "provider config path is not a directory",
        ));
    }
    Ok(directory)
}

fn provider_config_file_name(path: &Path) -> Result<&str, CliError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::unavailable("invalid provider config path"))
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
    read_provider_config_from_dir(provider, Path::new(PROVIDER_CONFIG_DIR))
}

fn read_provider_config_from_dir(
    provider: &str,
    dir: &Path,
) -> Result<CtxProviderConfig, CliError> {
    let directory = open_provider_config_dir(dir)?;
    let entries = fs::read_dir(provider_config_proc_fd_path(&directory)).map_err(|error| {
        CliError::unavailable(format!("cannot read provider config dir: {error}"))
    })?;
    for entry in entries {
        let entry = entry
            .map_err(|error| CliError::unavailable(format!("cannot read provider config: {error}")))?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        if Path::new(file_name)
            .extension()
            .and_then(|value| value.to_str())
            != Some("json")
        {
            continue;
        }
        let content = read_provider_config_file_at(&directory, file_name)?;
        let config = serde_json::from_str::<CtxProviderConfig>(&content)
            .map_err(|error| CliError::usage(format!("invalid provider config: {error}")))?;
        if cortexfs::provider_name_from_config(&config.base_url, config.name.as_deref())
            .as_deref()
            == Ok(provider)
        {
            return Ok(config);
        }
    }
    Err(CliError::usage(format!("missing provider: {provider}")))
}

fn provider_config_proc_fd_path(directory: &fs::File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()))
}

fn read_provider_config_file_at(parent_dir: &fs::File, file_name: &str) -> Result<String, CliError> {
    let file_fd = nix::fcntl::openat(
        parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|error| CliError::unavailable(format!("cannot read provider config: {error}")))?;
    read_provider_config_open_file(fs::File::from(file_fd), "provider config")
}

#[cfg(test)]
fn read_provider_config_file(path: &Path) -> Result<String, CliError> {
    let Some(parent) = path.parent() else {
        return Err(CliError::unavailable("provider config path has no parent"));
    };
    let parent_dir = open_provider_config_dir(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::unavailable("provider config path has no file name"))?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|error| {
        CliError::unavailable(format!("cannot read provider config {}: {error}", path.display()))
    })?;
    read_provider_config_open_file(fs::File::from(file_fd), &path.display().to_string())
}

fn read_provider_config_open_file(mut file: fs::File, label: &str) -> Result<String, CliError> {
    let metadata = file.metadata().map_err(|error| {
        CliError::unavailable(format!("cannot inspect provider config {label}: {error}"))
    })?;
    if !metadata.is_file() {
        return Err(CliError::unavailable(format!(
            "provider config is not a regular file: {label}",
        )));
    }
    if metadata.len() > MAX_CTX_PROVIDER_CONFIG_BYTES {
        return Err(CliError::unavailable(format!(
            "provider config is too large: {label}",
        )));
    }
    let len = usize::try_from(metadata.len())
        .map_err(|_error| CliError::unavailable("provider config is too large"))?;
    let mut content = vec![0; len];
    file.read_exact(&mut content).map_err(|error| {
        CliError::unavailable(format!("cannot read provider config {label}: {error}"))
    })?;
    String::from_utf8(content)
        .map_err(|_error| CliError::usage(format!("provider config is not utf-8: {label}")))
}

fn exchange_oauth_token(token_url: &str, form: &str) -> Result<cortexfs::OAuthTokenResponse, CliError> {
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

fn ctx_provider_curl_command() -> ProcessCommand {
    let mut command = ProcessCommand::new(CTX_PROVIDER_CURL_BIN);
    command.env_clear().arg("-q").arg("--config").arg("-");
    command
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

fn read_oauth_callback_request(
    stream: &mut std::net::TcpStream,
    deadline: std::time::Instant,
) -> Result<String, CliError> {
    let read_timeout = deadline
        .checked_duration_since(std::time::Instant::now())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| CliError::unavailable("oauth callback timed out"))?;
    stream
        .set_read_timeout(Some(read_timeout))
        .map_err(|error| CliError::unavailable(format!("cannot configure oauth callback: {error}")))?;
    read_oauth_callback_request_from_reader(stream, MAX_OAUTH_CALLBACK_REQUEST_BYTES)
}

fn read_oauth_callback_request_from_reader(
    mut reader: impl Read,
    max_bytes: usize,
) -> Result<String, CliError> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let size = reader
            .read(&mut chunk)
            .map_err(|error| match error.kind() {
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => {
                    CliError::unavailable("oauth callback timed out")
                }
                _ => CliError::unavailable(format!("cannot read oauth callback: {error}")),
            })?;
        if size == 0 {
            break;
        }
        let Some(read_bytes) = chunk.get(..size) else {
            return Err(CliError::unavailable("oauth callback exceeded buffer"));
        };
        bytes.extend_from_slice(read_bytes);
        if bytes.len() > max_bytes {
            return Err(CliError::unavailable("oauth callback exceeded buffer"));
        }
        if let Some(end) = oauth_callback_headers_end(&bytes) {
            bytes.truncate(end);
            break;
        }
    }
    String::from_utf8(bytes)
        .map_err(|_error| CliError::usage("oauth callback must be valid UTF-8"))
}

fn oauth_callback_headers_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .or_else(|| {
            bytes.windows(2)
                .position(|window| window == b"\n\n")
                .map(|position| position + 2)
        })
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
    let version = fields.next().unwrap_or_default();
    if method != "GET" {
        return Err(CliError::usage("oauth callback must use GET"));
    }
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") || fields.next().is_some() {
        return Err(CliError::usage("oauth callback request line is invalid"));
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
            "code" if value.is_empty() => {
                return Err(CliError::usage("oauth callback empty code"));
            }
            "code" if code.is_none() => code = Some(value),
            "code" => return Err(CliError::usage("oauth callback repeated code")),
            "state" if value.is_empty() => {
                return Err(CliError::usage("oauth callback empty state"));
            }
            "state" if state.is_none() => state = Some(value),
            "state" => return Err(CliError::usage("oauth callback repeated state")),
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

fn is_provider_name(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('/')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn curl_config_quote(value: &str) -> Result<String, CliError> {
    let mut quoted = String::from("\"");
    for character in value.chars() {
        if character.is_ascii_control() {
            return Err(CliError::usage(
                "curl config value contains a forbidden control character",
            ));
        }
        if matches!(character, '"' | '\\') {
            quoted.push('\\');
        }
        quoted.push(character);
    }
    quoted.push('"');
    Ok(quoted)
}

fn terminate_process_child(child: &mut std::process::Child) {
    match child.kill() {
        Ok(()) | Err(_) => {}
    }
    match child.wait() {
        Ok(_) | Err(_) => {}
    }
}
