use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::env;
use std::fmt::Write as FmtWrite;

/// OAuth 2.0 provider configuration used outside the stable `/ctx` ABI.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct OAuthProviderConfig {
    pub client_id: String,
    pub auth_url: String,
    pub token_url: String,
    pub redirect_uri: String,
    #[serde(default)]
    pub scopes: Vec<String>,
    pub access_token_account: Option<String>,
    pub refresh_token_account: Option<String>,
}

/// PKCE verifier/challenge pair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthPkce {
    verifier: String,
    challenge: String,
}

/// Parsed OAuth bearer token response.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub token_type: Option<String>,
    #[serde(default)]
    pub expires_in: Option<u64>,
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

/// Error while building or resolving OAuth data.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OAuthError {
    InvalidConfig,
    InvalidVerifier,
    InvalidToken,
    KeychainUnavailable,
}

impl OAuthProviderConfig {
    /// Returns the system keychain account used for OAuth access tokens.
    #[must_use]
    pub fn access_account(&self) -> &str {
        self.access_token_account
            .as_deref()
            .unwrap_or("oauth:access")
    }

    /// Returns the system keychain account used for OAuth refresh tokens.
    #[must_use]
    pub fn refresh_account(&self) -> &str {
        self.refresh_token_account
            .as_deref()
            .unwrap_or("oauth:refresh")
    }
}

impl OAuthPkce {
    /// Builds a PKCE pair from an already-generated verifier.
    pub fn from_verifier(verifier: &str) -> Result<Self, OAuthError> {
        if !is_valid_pkce_verifier(verifier) {
            return Err(OAuthError::InvalidVerifier);
        }
        let digest = Sha256::digest(verifier.as_bytes());
        Ok(Self {
            verifier: verifier.to_owned(),
            challenge: base64_url_no_pad(&digest),
        })
    }

    /// Builds a deterministic PKCE pair from entropy bytes.
    ///
    /// Callers that need fresh entropy should pass at least 32 bytes from the
    /// OS random source. This function is deterministic to keep the core
    /// testable and independent from a concrete RNG.
    pub fn from_entropy(entropy: &[u8]) -> Result<Self, OAuthError> {
        if entropy.len() < 32 {
            return Err(OAuthError::InvalidVerifier);
        }
        let verifier = base64_url_no_pad(entropy);
        Self::from_verifier(&verifier)
    }

    #[must_use]
    pub fn verifier(&self) -> &str {
        &self.verifier
    }

    #[must_use]
    pub fn challenge(&self) -> &str {
        &self.challenge
    }

    #[must_use]
    pub const fn method() -> &'static str {
        "S256"
    }
}

/// Builds an OAuth authorization URL using Authorization Code + PKCE.
pub fn oauth_authorization_url(
    config: &OAuthProviderConfig,
    state: &str,
    pkce: &OAuthPkce,
) -> Result<String, OAuthError> {
    if !is_valid_oauth_config(config) || state.is_empty() || has_ascii_control(state) {
        return Err(OAuthError::InvalidConfig);
    }
    let mut query = vec![
        ("response_type", "code".to_owned()),
        ("client_id", config.client_id.clone()),
        ("redirect_uri", config.redirect_uri.clone()),
        ("state", state.to_owned()),
        ("code_challenge", pkce.challenge().to_owned()),
        ("code_challenge_method", OAuthPkce::method().to_owned()),
    ];
    if !config.scopes.is_empty() {
        query.push(("scope", config.scopes.join(" ")));
    }
    Ok(append_query(&config.auth_url, &query))
}

/// Builds the form body for exchanging an authorization code for tokens.
pub fn oauth_authorization_code_form(
    config: &OAuthProviderConfig,
    code: &str,
    pkce: &OAuthPkce,
) -> Result<String, OAuthError> {
    if !is_valid_oauth_config(config) || code.is_empty() || has_ascii_control(code) {
        return Err(OAuthError::InvalidConfig);
    }
    Ok(form_urlencoded(&[
        ("grant_type", "authorization_code"),
        ("client_id", &config.client_id),
        ("code", code),
        ("redirect_uri", &config.redirect_uri),
        ("code_verifier", pkce.verifier()),
    ]))
}

/// Builds the form body for refreshing an OAuth access token.
pub fn oauth_refresh_token_form(
    config: &OAuthProviderConfig,
    refresh_token: &str,
) -> Result<String, OAuthError> {
    if !is_valid_oauth_config(config)
        || refresh_token.trim().is_empty()
        || has_ascii_control(refresh_token)
    {
        return Err(OAuthError::InvalidConfig);
    }
    Ok(form_urlencoded(&[
        ("grant_type", "refresh_token"),
        ("client_id", &config.client_id),
        ("refresh_token", refresh_token),
    ]))
}

/// Parses a token endpoint JSON response and validates bearer semantics.
pub fn parse_oauth_token_response(body: &[u8]) -> Result<OAuthTokenResponse, OAuthError> {
    let response = serde_json::from_slice::<OAuthTokenResponse>(body)
        .map_err(|_error| OAuthError::InvalidToken)?;
    if response.access_token.trim().is_empty() {
        return Err(OAuthError::InvalidToken);
    }
    if let Some(token_type) = response.token_type.as_deref()
        && !token_type.eq_ignore_ascii_case("bearer")
    {
        return Err(OAuthError::InvalidToken);
    }
    Ok(response)
}

/// Resolves an OAuth access token with generated environment before system keychain.
pub fn resolve_oauth_access_token(
    provider: &str,
    config: &OAuthProviderConfig,
) -> Result<Option<String>, OAuthError> {
    resolve_oauth_access_token_with(
        provider,
        config,
        |name| env::var(name),
        oauth_keychain_secret,
    )
}

/// Testable OAuth token resolution core.
pub fn resolve_oauth_access_token_with<E, K>(
    provider: &str,
    config: &OAuthProviderConfig,
    env_lookup: E,
    keychain_lookup: K,
) -> Result<Option<String>, OAuthError>
where
    E: Fn(&str) -> Result<String, env::VarError>,
    K: FnOnce(&str, &str) -> Result<Option<String>, OAuthError>,
{
    if provider.is_empty() || has_ascii_control(provider) || !is_valid_oauth_config(config) {
        return Err(OAuthError::InvalidConfig);
    }
    let name = crate::provider_oauth_access_token_env_name(provider);
    if !is_valid_env_key(&name) {
        return Err(OAuthError::InvalidConfig);
    }
    match env_lookup(&name) {
        Ok(value) if !value.trim().is_empty() => return Ok(Some(value)),
        Ok(_value) => {}
        Err(env::VarError::NotPresent) => {}
        Err(env::VarError::NotUnicode(_value)) => return Err(OAuthError::InvalidConfig),
    }
    keychain_lookup(&oauth_keychain_service(provider), config.access_account())
}

#[must_use]
fn oauth_keychain_service(provider: &str) -> String {
    format!("cortexfs:{provider}")
}

fn oauth_keychain_secret(service: &str, account: &str) -> Result<Option<String>, OAuthError> {
    let entry = match keyring::Entry::new(service, account) {
        Ok(entry) => entry,
        Err(keyring::Error::NoDefaultStore) => return Ok(None),
        Err(_error) => return Err(OAuthError::KeychainUnavailable),
    };
    let secret = match entry.get_password() {
        Ok(secret) => secret,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(_error) => return Err(OAuthError::KeychainUnavailable),
    };
    if secret.is_empty() {
        Ok(None)
    } else {
        Ok(Some(secret))
    }
}

fn is_valid_oauth_config(config: &OAuthProviderConfig) -> bool {
    !config.client_id.is_empty()
        && !config.auth_url.is_empty()
        && !config.token_url.is_empty()
        && !config.redirect_uri.is_empty()
        && !config
            .scopes
            .iter()
            .any(|scope| scope.trim().is_empty() || has_ascii_control(scope))
}

fn has_ascii_control(value: &str) -> bool {
    value.bytes().any(|byte| byte.is_ascii_control())
}

fn is_valid_pkce_verifier(verifier: &str) -> bool {
    (43..=128).contains(&verifier.len())
        && verifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~'))
}

fn is_valid_env_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

fn append_query(base: &str, pairs: &[(&str, String)]) -> String {
    let separator = if base.contains('?') { '&' } else { '?' };
    format!("{base}{separator}{}", form_urlencoded_owned(pairs))
}

fn form_urlencoded(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|&(key, value)| format!("{}={}", url_encode(key), url_encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn form_urlencoded_owned(pairs: &[(&str, String)]) -> String {
    pairs
        .iter()
        .map(|pair| format!("{}={}", url_encode(pair.0), url_encode(&pair.1)))
        .collect::<Vec<_>>()
        .join("&")
}

fn url_encode(value: &str) -> String {
    let mut output = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            output.push(char::from(byte));
        } else {
            let _ignored = write!(output, "%{byte:02X}");
        }
    }
    output
}

fn base64_url_no_pad(bytes: &[u8]) -> String {
    let mut output = String::new();
    for chunk in bytes.chunks(3) {
        let Some(&b0) = chunk.first() else {
            continue;
        };
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        output.push(base64_url_char(usize::from(b0 >> 2)));
        output.push(base64_url_char(usize::from(
            ((b0 & 0b0000_0011) << 4) | (b1 >> 4),
        )));
        if chunk.len() > 1 {
            output.push(base64_url_char(usize::from(
                ((b1 & 0b0000_1111) << 2) | (b2 >> 6),
            )));
        }
        if chunk.len() > 2 {
            output.push(base64_url_char(usize::from(b2 & 0b0011_1111)));
        }
    }
    output
}

fn base64_url_char(index: usize) -> char {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    char::from(TABLE.get(index).copied().unwrap_or(b'A'))
}
