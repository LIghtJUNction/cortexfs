use super::*;
use std::fs;
use std::io;
use std::path::Path;

pub(crate) fn provider_credential(
    provider: &str,
    config: &RunnerProviderConfig,
    key_slot: Option<&str>,
    driver: ProviderRuntimeDriver,
    transport: &ResolvedTransport,
) -> Result<Option<ProviderCredential>, String> {
    let methods = config.auth_methods();
    let codex = provider == "codex"
        || (methods
            .iter()
            .any(|method| method.method == cortexfs::AuthMethod::OAuth)
            && config
                .oauth
                .as_ref()
                .is_some_and(cortexfs::OAuthProviderConfig::is_codex));
    if codex && driver != ProviderRuntimeDriver::OpenAiResponses {
        return Err("Codex OAuth only supports openai.responses".to_owned());
    }
    if env::var_os(cortexfs::runtime::egress::PROVIDER_EGRESS_DIR_ENV).as_deref()
        == Some(std::ffi::OsStr::new(
            cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH,
        ))
    {
        if !matches!(transport, ResolvedTransport::Unix { socket_path, .. }
            if socket_path == &format!("{}/{provider}.sock", cortexfs::runtime::egress::PROVIDER_EGRESS_SANDBOX_PATH))
        {
            return Err("provider route bypasses the authenticated egress socket".to_owned());
        }
        let token = env::var(cortexfs::runtime::egress::PROVIDER_EGRESS_TOKEN_ENV)
            .map_err(|_error| "provider egress capability unavailable".to_owned())?;
        return Ok(Some(ProviderCredential::Egress { token, codex }));
    }
    let account = key_slot
        .map(str::to_owned)
        .or_else(|| config.api_key_slot())
        .unwrap_or_else(|| "default".to_owned());
    let mut runtime =
        provider_secret_from_runtime_value_with_env(provider, &account, |name| env::var(name));
    if runtime.is_none() {
        runtime =
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
    }
    if let Some(token) = runtime {
        return if codex {
            env::var("CTX_PROVIDER_SECRET_ACCOUNT_ID")
                .ok()
                .filter(|value| !value.trim().is_empty())
                .map(|account_id| Some(ProviderCredential::Codex { token, account_id }))
                .ok_or_else(|| "runtime Codex account id unavailable".to_owned())
        } else if methods
            .iter()
            .any(|method| method.method == cortexfs::AuthMethod::ApiKey)
        {
            Ok(Some(api_key_credential(&token, driver)))
        } else {
            Ok(Some(ProviderCredential::Bearer(token)))
        };
    }
    let adapter =
        cortexfs::configured_adapter(provider, &config.base_url, methods, config.oauth.clone())
            .ok_or_else(|| format!("provider auth adapter unavailable: {provider}"))?;
    match crate::provider::auth::resolve_credential(
        adapter.as_ref(),
        config.oauth.as_ref(),
        key_slot.unwrap_or("default"),
    ) {
        Ok(credential) => credential.map_or(Ok(None), |credential| {
            profile_credential(&credential, driver, codex)
        }),
        Err(error)
            if anonymous_store_error(
                error,
                key_slot,
                transport,
                nix::unistd::geteuid().as_raw(),
            ) =>
        {
            Ok(None)
        }
        Err(error) => Err(format!("{provider}: {error}")),
    }
}

fn anonymous_store_error(
    error: cortexfs::AuthProviderError,
    key_slot: Option<&str>,
    transport: &ResolvedTransport,
    uid: u32,
) -> bool {
    error == cortexfs::AuthProviderError::StoreAccessDenied
        && uid != 0
        && key_slot.is_none()
        && transport_allows_unauthenticated(transport)
}

fn codex_credential(token: String) -> Option<ProviderCredential> {
    cortexfs::oauth_account_id(&token)
        .map(|account_id| ProviderCredential::Codex { token, account_id })
}

fn profile_credential(
    credential: &cortexfs::Credential,
    driver: ProviderRuntimeDriver,
    codex: bool,
) -> Result<Option<ProviderCredential>, String> {
    match *credential {
        cortexfs::Credential::ApiKey { ref key, .. } => Ok(Some(api_key_credential(key, driver))),
        cortexfs::Credential::OAuth {
            ref access_token, ..
        } if codex => {
            if driver != ProviderRuntimeDriver::OpenAiResponses {
                return Err("Codex OAuth only supports openai.responses".to_owned());
            }
            Ok(codex_credential(access_token.clone()))
        }
        cortexfs::Credential::OAuth {
            ref access_token, ..
        } => Ok(Some(ProviderCredential::Bearer(access_token.clone()))),
    }
}

fn api_key_credential(token: &str, driver: ProviderRuntimeDriver) -> ProviderCredential {
    match driver {
        ProviderRuntimeDriver::Anthropic => ProviderCredential::AnthropicApiKey(token.to_owned()),
        ProviderRuntimeDriver::Gemini => ProviderCredential::GoogleApiKey(token.to_owned()),
        ProviderRuntimeDriver::OpenAiChat | ProviderRuntimeDriver::OpenAiResponses => {
            ProviderCredential::Bearer(token.to_owned())
        }
    }
}

#[cfg(test)]
mod tests;

fn runtime_secret_env_matches(
    provider: &str,
    account: &str,
    get_env: &impl Fn(&str) -> Result<String, env::VarError>,
) -> bool {
    cortexfs::provider::auth::is_api_key_slot(account)
        && get_env("CTX_PROVIDER_SECRET_PROVIDER").as_deref() == Ok(provider)
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
