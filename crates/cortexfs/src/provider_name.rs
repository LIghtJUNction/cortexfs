use std::fs;
use std::fs::File;
use std::io::Write as _;
use std::net::IpAddr;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use nix::fcntl::{FcntlArg, FdFlag, fcntl};

const PROVIDER_SYSTEM_SECRET_ROOT: &str = "/var/lib/cortexfs/secrets/provider";

/// Error returned when a provider config cannot produce a stable provider name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderNameError {
    /// The provider base URL has no usable host.
    MissingHost,
    /// A local/IP endpoint needs an explicit stable provider name.
    MissingNameForAddress,
    /// The configured provider name is not a `CortexFS` object name.
    InvalidName,
}

/// Returns the stable `CortexFS` provider name for a provider config.
///
/// Official providers use short canonical names when `name` is omitted.
/// Non-official domain providers keep their domain name by default. IP and
/// localhost endpoints must set `name` so `/ctx/model/<provider>` remains a
/// stable object path rather than an address literal.
pub fn provider_name_from_config(
    base_url: &str,
    name: Option<&str>,
) -> Result<String, ProviderNameError> {
    if let Some(name) = name.map(str::trim).filter(|value| !value.is_empty()) {
        return if crate::is_object_name(name) {
            Ok(name.to_owned())
        } else {
            Err(ProviderNameError::InvalidName)
        };
    }

    let host = provider_host_from_base_url(base_url).ok_or(ProviderNameError::MissingHost)?;
    if provider_host_requires_name(&host) {
        return Err(ProviderNameError::MissingNameForAddress);
    }

    Ok(canonical_provider_name_from_host(&host).to_owned())
}

/// Returns the stable `CortexFS` provider name for a provider base URL.
///
/// Prefer `provider_name_from_config` for provider JSON. This lower-level
/// helper is kept for callers that only need host canonicalization.
#[must_use]
pub fn provider_name_from_base_url(base_url: &str) -> Option<String> {
    let host = provider_host_from_base_url(base_url)?;
    Some(canonical_provider_name_from_host(&host).to_owned())
}

/// Returns the lowercase host from a provider base URL.
#[must_use]
pub fn provider_host_from_base_url(base_url: &str) -> Option<String> {
    let mut rest = base_url.trim();
    if let Some(value) = rest.strip_prefix("https://") {
        rest = value;
    } else if let Some(value) = rest.strip_prefix("http://") {
        rest = value;
    }
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default();
    let host = authority
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

/// Returns the generated OAuth access-token environment variable for a provider.
#[must_use]
pub fn provider_oauth_access_token_env_name(provider: &str) -> String {
    format!("CTX_{}_OAUTH_ACCESS_TOKEN", provider_env_label(provider))
}

/// Returns the generated OAuth refresh-token environment variable for a provider.
#[must_use]
pub fn provider_oauth_refresh_token_env_name(provider: &str) -> String {
    format!("CTX_{}_OAUTH_REFRESH_TOKEN", provider_env_label(provider))
}

/// Returns the system keychain service name for a provider.
#[must_use]
pub fn provider_keychain_service(provider: &str) -> String {
    format!("cortexfs:{provider}")
}

/// Stores a provider API secret in the root-owned `CortexFS` system secret store.
pub fn store_provider_system_secret(
    provider: &str,
    account: &str,
    secret: &str,
) -> Result<(), ProviderSystemSecretError> {
    let path = provider_system_secret_path(provider, account)?;
    let Some(parent) = path.parent() else {
        return Err(ProviderSystemSecretError::InvalidName);
    };
    fs::create_dir_all(parent).map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
    set_private_dir_permissions(Path::new("/var/lib/cortexfs/secrets"))?;
    set_private_dir_permissions(Path::new(PROVIDER_SYSTEM_SECRET_ROOT))?;
    set_private_dir_permissions(parent)?;
    let temp = path.with_extension("tmp");
    {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
        file.write_all(secret.as_bytes())
            .and_then(|()| file.write_all(b"\n"))
            .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
    }
    fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
    fs::rename(&temp, &path).map_err(|_error| ProviderSystemSecretError::CannotWrite)
}

/// Reads a provider API secret from the root-owned `CortexFS` system secret store.
pub fn read_provider_system_secret(
    provider: &str,
    account: &str,
) -> Result<Option<String>, ProviderSystemSecretError> {
    let path = provider_system_secret_path(provider, account)?;
    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_error) => return Err(ProviderSystemSecretError::CannotRead),
    };
    let secret = content.trim_end_matches(['\r', '\n']);
    if secret.is_empty() {
        Ok(None)
    } else {
        Ok(Some(secret.to_owned()))
    }
}

/// Opens a provider API secret and clears close-on-exec so a runtime child can
/// inherit it without exposing the secret in environment variables.
pub fn open_provider_system_secret(
    provider: &str,
    account: &str,
) -> Result<Option<ProviderSystemSecretHandle>, ProviderSystemSecretError> {
    let path = provider_system_secret_path(provider, account)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_error) => return Err(ProviderSystemSecretError::CannotRead),
    };
    clear_fd_cloexec(&file)?;
    Ok(Some(ProviderSystemSecretHandle {
        provider: provider.to_owned(),
        account: account.to_owned(),
        file,
    }))
}

/// Returns whether a provider API secret exists in the system secret store.
pub fn provider_system_secret_exists(
    provider: &str,
    account: &str,
) -> Result<bool, ProviderSystemSecretError> {
    provider_system_secret_path(provider, account).map(|path| path.is_file())
}

/// Error while reading or writing the `CortexFS` system provider secret store.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderSystemSecretError {
    /// Provider or account name is invalid.
    InvalidName,
    /// Secret could not be read.
    CannotRead,
    /// Secret could not be written.
    CannotWrite,
}

/// Open provider secret inherited by a runtime child via file descriptor.
#[derive(Debug)]
pub struct ProviderSystemSecretHandle {
    provider: String,
    account: String,
    file: File,
}

/// Provider secret material read before entering a reduced-privilege runtime.
#[derive(Debug)]
pub struct ProviderSystemSecret {
    provider: String,
    account: String,
    secret: String,
}

impl ProviderSystemSecret {
    #[must_use]
    pub fn provider(&self) -> &str {
        &self.provider
    }

    #[must_use]
    pub fn account(&self) -> &str {
        &self.account
    }

    #[must_use]
    pub fn secret(&self) -> &str {
        &self.secret
    }
}

impl ProviderSystemSecretHandle {
    /// Environment metadata for passing this already-open secret fd.
    ///
    /// These variables contain no secret material; they identify only an fd and
    /// the provider slot it belongs to.
    #[must_use]
    pub fn env(&self) -> [(String, String); 3] {
        [
            (
                "CTX_PROVIDER_SECRET_FD".to_owned(),
                self.file.as_raw_fd().to_string(),
            ),
            (
                "CTX_PROVIDER_SECRET_PROVIDER".to_owned(),
                self.provider.clone(),
            ),
            ("CTX_PROVIDER_SECRET_SLOT".to_owned(), self.account.clone()),
        ]
    }
}

fn canonical_provider_name_from_host(host: &str) -> &str {
    match host {
        "api.openai.com" => "openai",
        "api.anthropic.com" => "anthropic",
        "generativelanguage.googleapis.com" => "google",
        _ => host,
    }
}

fn provider_host_requires_name(host: &str) -> bool {
    host == "localhost" || host.parse::<IpAddr>().is_ok()
}

/// Opens the default provider system secret for a selected model alias/name.
pub fn open_provider_system_secret_for_model(
    ctx_root: &Path,
    model: &str,
) -> Result<Option<ProviderSystemSecretHandle>, ProviderSystemSecretError> {
    let Some(provider) = selected_model_provider(ctx_root, model) else {
        return Ok(None);
    };
    open_provider_system_secret(&provider, "default")
}

/// Reads the default provider system secret for a selected model alias/name.
pub fn read_provider_system_secret_for_model(
    ctx_root: &Path,
    model: &str,
) -> Result<Option<ProviderSystemSecret>, ProviderSystemSecretError> {
    let Some(provider) = selected_model_provider(ctx_root, model) else {
        return Ok(None);
    };
    let account = "default";
    let Some(secret) = read_provider_system_secret(&provider, account)? else {
        return Ok(None);
    };
    Ok(Some(ProviderSystemSecret {
        provider,
        account: account.to_owned(),
        secret,
    }))
}

fn selected_model_provider(ctx_root: &Path, model: &str) -> Option<String> {
    let model = model.trim();
    if model.contains('/') {
        return model.split_once('/').and_then(|(provider, model)| {
            (!provider.is_empty() && !model.is_empty()).then_some(provider.to_owned())
        });
    }
    if !matches!(model, "main" | "helper") {
        return None;
    }
    let target = fs::read_link(ctx_root.join("model").join(model)).ok()?;
    let target = target.to_string_lossy();
    let target = target
        .strip_prefix("/ctx/model/")
        .or_else(|| target.strip_prefix("model/"))
        .unwrap_or(&target);
    target.split_once('/').and_then(|(provider, model)| {
        (!provider.is_empty() && !model.is_empty()).then_some(provider.to_owned())
    })
}

fn provider_system_secret_path(
    provider: &str,
    account: &str,
) -> Result<std::path::PathBuf, ProviderSystemSecretError> {
    if !crate::is_object_name(provider) || !is_secret_account_name(account) {
        return Err(ProviderSystemSecretError::InvalidName);
    }
    Ok(Path::new(PROVIDER_SYSTEM_SECRET_ROOT)
        .join(provider)
        .join(account))
}

fn is_secret_account_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

fn set_private_dir_permissions(path: &Path) -> Result<(), ProviderSystemSecretError> {
    match fs::set_permissions(path, fs::Permissions::from_mode(0o700)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_error) => Err(ProviderSystemSecretError::CannotWrite),
    }
}

fn clear_fd_cloexec(file: &File) -> Result<(), ProviderSystemSecretError> {
    let flags =
        fcntl(file, FcntlArg::F_GETFD).map_err(|_error| ProviderSystemSecretError::CannotRead)?;
    let mut flags = FdFlag::from_bits_truncate(flags);
    flags.remove(FdFlag::FD_CLOEXEC);
    fcntl(file, FcntlArg::F_SETFD(flags))
        .map(|_value| ())
        .map_err(|_error| ProviderSystemSecretError::CannotRead)
}

fn provider_env_label(provider: &str) -> String {
    provider
        .bytes()
        .map(|byte| match byte {
            b'a'..=b'z' => char::from(byte.to_ascii_uppercase()),
            b'A'..=b'Z' | b'0'..=b'9' => char::from(byte),
            _ => '_',
        })
        .collect()
}
