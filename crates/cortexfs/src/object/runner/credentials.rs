use super::*;
use std::fs;
use std::io;
use std::path::Path;

pub(crate) fn provider_credential(
    provider: &str,
    config: &RunnerProviderConfig,
    key_slot: Option<&str>,
    driver: ProviderRuntimeDriver,
) -> Result<Option<ProviderCredential>, String> {
    let methods = config.auth_methods();
    let api_key_enabled = methods
        .iter()
        .any(|method| method.method == cortexfs::AuthMethod::ApiKey);
    let oauth_enabled = methods
        .iter()
        .any(|method| method.method == cortexfs::AuthMethod::OAuth);
    let oauth = config.oauth.as_ref();
    let codex = oauth_enabled && oauth.is_some_and(cortexfs::OAuthProviderConfig::is_codex);
    let account = key_slot
        .map(str::to_owned)
        .or_else(|| config.api_key_slot())
        .unwrap_or_else(|| "default".to_owned());
    let runtime =
        provider_secret_from_runtime_value_with_env(provider, &account, |name| env::var(name));
    if codex {
        if driver != ProviderRuntimeDriver::OpenAiResponses {
            return Err("Codex OAuth only supports openai.responses".to_owned());
        }
        if let Some(token) = runtime {
            return env::var("CTX_PROVIDER_SECRET_ACCOUNT_ID")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(|account_id| Some(ProviderCredential::Codex { token, account_id }))
                .ok_or_else(|| "runtime Codex account id unavailable".to_owned());
        }
        let Some(oauth) = oauth else { return Ok(None) };
        return if key_slot.is_none() {
            cortexfs::resolve_oauth_credential(provider, oauth)
                .map(|value| value.map(codex_credential))
                .map_err(|_error| format!("oauth credential unavailable: {provider}"))
        } else {
            Ok(None)
        };
    }
    if !api_key_enabled {
        if !oauth_enabled {
            return Ok(None);
        }
        return oauth
            .filter(|_| key_slot.is_none())
            .ok_or_else(|| format!("provider OAuth is not configured: {provider}"))
            .and_then(|oauth| {
                resolve_runtime_oauth(provider, config, oauth)
                    .map(|token| token.map(|(access, _account)| ProviderCredential::Bearer(access)))
                    .map_err(|_error| format!("oauth credential unavailable: {provider}"))
            });
    }
    let credential = |token| {
        if driver == ProviderRuntimeDriver::Anthropic {
            ProviderCredential::AnthropicApiKey(token)
        } else {
            ProviderCredential::Bearer(token)
        }
    };
    if let Some(token) = runtime {
        return Ok(Some(credential(token)));
    }
    let runtime =
        provider_secret_from_runtime_file_with_env(provider, &account, |name| env::var(name))
            .and_then(|value| {
                value.map_or_else(
                    || {
                        provider_secret_from_inherited_fd_with_env(provider, &account, |name| {
                            env::var(name)
                        })
                    },
                    |value| Ok(Some(value)),
                )
            })
            .map_err(|_error| format!("runtime provider secret unavailable: {provider}"))?;
    if let Some(token) = runtime {
        return Ok(Some(credential(token)));
    }
    match cortexfs::read_provider_system_secret(provider, &account) {
        Ok(Some(token)) => return Ok(Some(credential(token))),
        Ok(None) | Err(cortexfs::ProviderSystemSecretError::CannotRead) => {}
        Err(_error) => return Err(format!("system provider secret unavailable: {provider}")),
    }
    let Some(oauth) = oauth.filter(|_| oauth_enabled) else {
        return Ok(None);
    };
    if key_slot.is_none() {
        return resolve_runtime_oauth(provider, config, oauth)
            .map(|token| token.map(|(access, _account)| ProviderCredential::Bearer(access)))
            .map_err(|_error| format!("oauth credential unavailable: {provider}"));
    }
    Ok(None)
}
fn codex_credential((token, account_id): cortexfs::OAuthCredential) -> ProviderCredential {
    ProviderCredential::Codex { token, account_id }
}

fn resolve_runtime_oauth(
    provider: &str,
    config: &RunnerProviderConfig,
    oauth: &cortexfs::OAuthProviderConfig,
) -> Result<Option<(String, String)>, String> {
    let adapter = cortexfs::configured_adapter(
        provider,
        &config.base_url,
        config.auth_methods(),
        Some(oauth.clone()),
    )
    .ok_or_else(|| format!("provider OAuth adapter unavailable: {provider}"))?;
    cortexfs::resolve_oauth_credential_with(provider, oauth, |request| {
        cortexfs::refresh_oauth_result(provider, request, adapter.as_ref())
    })
    .map_err(|_error| format!("oauth credential unavailable: {provider}"))
}

fn runtime_secret_env_matches(
    provider: &str,
    account: &str,
    get_env: &impl Fn(&str) -> Result<String, env::VarError>,
) -> bool {
    get_env("CTX_PROVIDER_SECRET_PROVIDER").as_deref() == Ok(provider)
        && get_env("CTX_PROVIDER_SECRET_SLOT").as_deref() == Ok(account)
}
pub(crate) fn provider_secret_from_runtime_value_with_env(
    provider: &str,
    account: &str,
    get_env: impl Fn(&str) -> Result<String, env::VarError>,
) -> Option<String> {
    if !runtime_secret_env_matches(provider, account, &get_env) {
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
pub(crate) fn provider_secret_from_runtime_file_with_env(
    provider: &str,
    account: &str,
    get_env: impl Fn(&str) -> Result<String, env::VarError>,
) -> Result<Option<String>, io::Error> {
    if !runtime_secret_env_matches(provider, account, &get_env) {
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
    Ok(nonempty_secret(&read_runtime_provider_secret_file(path)?))
}
pub(crate) fn read_runtime_provider_secret_file(path: &Path) -> Result<String, io::Error> {
    read_runtime_secret(
        open_regular_file_no_follow(path, nix::fcntl::OFlag::O_CLOEXEC)?,
        "runtime provider secret file is invalid",
    )
}
fn read_runtime_secret(mut file: fs::File, invalid: &str) -> Result<String, io::Error> {
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_RUNTIME_PROVIDER_SECRET_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, invalid));
    }
    let len = usize::try_from(metadata.len())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    read_utf8_exact_len(&mut file, len)
}
pub(crate) fn provider_secret_from_inherited_fd_with_env(
    provider: &str,
    account: &str,
    get_env: impl Fn(&str) -> Result<String, env::VarError>,
) -> Result<Option<String>, io::Error> {
    if !runtime_secret_env_matches(provider, account, &get_env) {
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
    Ok(nonempty_secret(&read_runtime_secret(
        fs::File::open(format!("/proc/self/fd/{fd}"))?,
        "inherited provider secret fd is invalid",
    )?))
}
fn nonempty_secret(secret: &str) -> Option<String> {
    let secret = secret.trim_end_matches(['\r', '\n']);
    (!secret.is_empty()).then(|| secret.to_owned())
}
pub(crate) fn read_utf8_exact_len(file: &mut fs::File, len: usize) -> Result<String, io::Error> {
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.utf8_error()))
}
