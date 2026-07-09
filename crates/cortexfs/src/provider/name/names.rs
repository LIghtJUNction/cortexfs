use super::model_selection::{canonical_provider_name_from_host, provider_host_requires_name};
use super::secret_files::provider_env_label;
use crate::*;

pub(crate) const PROVIDER_SYSTEM_SECRET_ROOT: &str = "/var/lib/cortexfs/secrets/provider";

pub(crate) const MAX_PROVIDER_SYSTEM_SECRET_BYTES: u64 = 64 * 1024;

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
        return if is_object_name(name) {
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
    if rest.bytes().any(|byte| byte.is_ascii_control()) {
        return None;
    }
    if let Some(value) = rest.strip_prefix("https://") {
        rest = value;
    } else if let Some(value) = rest.strip_prefix("http://") {
        rest = value;
    } else {
        return None;
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
