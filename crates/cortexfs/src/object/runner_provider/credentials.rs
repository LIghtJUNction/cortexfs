macro_rules! runtime_secret_env_matches {
    ($provider:expr, $account:expr, $get_env:expr) => {
        $get_env("CTX_PROVIDER_SECRET_PROVIDER").as_deref() == Ok($provider)
            && $get_env("CTX_PROVIDER_SECRET_SLOT").as_deref() == Ok($account)
    };
}

fn provider_credential(
    provider: &str,
    config: &RunnerProviderConfig,
    key_slot: Option<&str>,
    driver: ProviderRuntimeDriver,
) -> Result<Option<ProviderCredential>, String> {
    macro_rules! credential_from_secret {
        ($api_key:expr) => {
            match driver {
                ProviderRuntimeDriver::AnthropicMessages => {
                    ProviderCredential::AnthropicApiKey($api_key)
                }
                ProviderRuntimeDriver::OpenAiChat | ProviderRuntimeDriver::OpenAiResponses => {
                    ProviderCredential::Bearer($api_key)
                }
            }
        };
    }

    let account = key_slot.unwrap_or("default");
    if let Some(api_key) =
        provider_secret_from_runtime_value_with_env(provider, account, |name| env::var(name))
    {
        return Ok(Some(credential_from_secret!(api_key)));
    }
    if let Some(api_key) =
        provider_secret_from_runtime_file_with_env(provider, account, |name| env::var(name))
        .map_err(|_error| format!("runtime provider secret unavailable: {provider}"))?
    {
        return Ok(Some(credential_from_secret!(api_key)));
    }
    if let Some(api_key) =
        provider_secret_from_inherited_fd_with_env(provider, account, |name| env::var(name))
        .map_err(|_error| format!("inherited provider secret unavailable: {provider}"))?
    {
        return Ok(Some(credential_from_secret!(api_key)));
    }
    match cortexfs::read_provider_system_secret(provider, account) {
        Ok(Some(api_key)) => {
            return Ok(Some(credential_from_secret!(api_key)));
        }
        Ok(None) | Err(cortexfs::ProviderSystemSecretError::CannotRead) => {}
        Err(_error) => return Err(format!("system provider secret unavailable: {provider}")),
    }
    let Some(oauth) = config.oauth.as_ref() else {
        return Ok(None);
    };
    if key_slot.is_none() {
        return cortexfs::resolve_oauth_access_token(provider, oauth)
            .map(|token| token.map(ProviderCredential::Bearer))
            .map_err(|_error| format!("oauth credential unavailable: {provider}"));
    }
    Ok(None)
}

fn provider_secret_from_runtime_value_with_env(
    provider: &str,
    account: &str,
    get_env: impl Fn(&str) -> Result<String, env::VarError>,
) -> Option<String> {
    if !runtime_secret_env_matches!(provider, account, get_env) {
        return None;
    }
    let secret = get_env("CTX_PROVIDER_SECRET_VALUE").ok()?;
    let secret = secret.trim_end_matches(['\r', '\n']);
    if secret.is_empty() {
        None
    } else {
        Some(secret.to_owned())
    }
}

fn provider_secret_from_runtime_file_with_env(
    provider: &str,
    account: &str,
    get_env: impl Fn(&str) -> Result<String, env::VarError>,
) -> Result<Option<String>, io::Error> {
    if !runtime_secret_env_matches!(provider, account, get_env) {
        return Ok(None);
    }
    let Ok(path) = get_env("CTX_PROVIDER_SECRET_PATH") else {
        return Ok(None);
    };
    if path.is_empty() {
        return Ok(None);
    }
    let path = Path::new(&path);
    if !path.is_absolute() {
        return Ok(None);
    }
    let secret = read_runtime_provider_secret_file(path)?;
    let secret = secret.trim_end_matches(['\r', '\n']);
    if secret.is_empty() {
        Ok(None)
    } else {
        Ok(Some(secret.to_owned()))
    }
}

fn read_runtime_provider_secret_file(path: &Path) -> Result<String, io::Error> {
    let mut file = open_regular_file_no_follow(path, nix::fcntl::OFlag::O_CLOEXEC)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_RUNTIME_PROVIDER_SECRET_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "runtime provider secret file is invalid",
        ));
    }
    let len = usize::try_from(metadata.len())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    read_utf8_exact_len(&mut file, len)
}

fn provider_secret_from_inherited_fd_with_env(
    provider: &str,
    account: &str,
    get_env: impl Fn(&str) -> Result<String, env::VarError>,
) -> Result<Option<String>, io::Error> {
    if !runtime_secret_env_matches!(provider, account, get_env) {
        return Ok(None);
    }
    let Ok(fd) = get_env("CTX_PROVIDER_SECRET_FD") else {
        return Ok(None);
    };
    if fd.is_empty() || !fd.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(None);
    }
    let fd = fd
        .parse::<i32>()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    if fd <= libc::STDERR_FILENO {
        return Ok(None);
    }
    let mut file = fs::File::open(format!("/proc/self/fd/{fd}"))?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_RUNTIME_PROVIDER_SECRET_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "inherited provider secret fd is invalid",
        ));
    }
    let len = usize::try_from(metadata.len())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    let secret = read_utf8_exact_len(&mut file, len)?;
    let secret = secret.trim_end_matches(['\r', '\n']);
    if secret.is_empty() {
        Ok(None)
    } else {
        Ok(Some(secret.to_owned()))
    }
}

fn read_utf8_exact_len(file: &mut fs::File, len: usize) -> Result<String, io::Error> {
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.utf8_error()))
}
