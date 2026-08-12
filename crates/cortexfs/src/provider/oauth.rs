use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::env;

pub const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const CODEX_DEVICE_USER_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
pub const CODEX_DEVICE_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
pub const CODEX_DEVICE_VERIFY_URL: &str = "https://auth.openai.com/codex/device";
pub const CODEX_DEVICE_REDIRECT_URI: &str = "https://auth.openai.com/deviceauth/callback";
const CODEX_SYSTEM_SLOTS: [&str; 4] =
    ["default", "oauth-refresh", "oauth-account", "oauth-expires"];
const OAUTH_EXPIRES_ACCOUNT: &str = "oauth:expires";

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthPkce {
    verifier: String,
    challenge: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub token_type: Option<String>,
    pub expires_in: Option<u64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
    pub id_token: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OAuthTokenState {
    pub access_token: String,
    pub refresh_token: String,
    pub account_id: String,
    pub expires_at: u64,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
pub struct DeviceCode {
    #[serde(rename = "device_auth_id")]
    pub id: String,
    #[serde(rename = "user_code")]
    pub code: String,
    pub interval: String,
}

#[derive(Deserialize)]
struct DeviceGrant {
    authorization_code: String,
    code_verifier: String,
    code_challenge: Option<String>,
}

pub type OAuthCredential = (String, String);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OAuthError {
    InvalidConfig,
    InvalidVerifier,
    InvalidToken,
    KeychainUnavailable,
    SystemStoreUnavailable,
    Transport,
}

impl OAuthProviderConfig {
    #[must_use]
    pub fn access_account(&self) -> &str {
        self.access_token_account
            .as_deref()
            .unwrap_or("oauth:access")
    }

    #[must_use]
    pub fn refresh_account(&self) -> &str {
        self.refresh_token_account
            .as_deref()
            .unwrap_or("oauth:refresh")
    }

    #[must_use]
    pub fn is_codex(&self) -> bool {
        self.client_id == CODEX_CLIENT_ID
    }
}

#[must_use]
pub fn codex_oauth_config() -> OAuthProviderConfig {
    OAuthProviderConfig {
        client_id: CODEX_CLIENT_ID.to_owned(),
        auth_url: "https://auth.openai.com/oauth/authorize".to_owned(),
        token_url: "https://auth.openai.com/oauth/token".to_owned(),
        redirect_uri: "http://localhost:1455/auth/callback".to_owned(),
        scopes: vec![
            "openid profile email offline_access api.connectors.read api.connectors.invoke"
                .to_owned(),
        ],
        access_token_account: None,
        refresh_token_account: None,
    }
}

impl OAuthPkce {
    pub fn from_verifier(verifier: &str) -> Result<Self, OAuthError> {
        if !(43..=128).contains(&verifier.len())
            || !verifier.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~')
            })
        {
            return Err(OAuthError::InvalidVerifier);
        }
        let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(Sha256::digest(verifier.as_bytes()));
        Ok(Self {
            verifier: verifier.to_owned(),
            challenge,
        })
    }

    pub fn from_entropy(entropy: &[u8]) -> Result<Self, OAuthError> {
        if entropy.len() < 32 {
            return Err(OAuthError::InvalidVerifier);
        }
        Self::from_verifier(&base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(entropy))
    }

    #[must_use]
    pub fn verifier(&self) -> &str {
        &self.verifier
    }
    #[must_use]
    pub fn challenge(&self) -> &str {
        &self.challenge
    }
}

pub fn oauth_authorization_url(
    config: &OAuthProviderConfig,
    state: &str,
    pkce: &OAuthPkce,
) -> Result<String, OAuthError> {
    if !valid_config(config) || state.is_empty() || controls(state) {
        return Err(OAuthError::InvalidConfig);
    }
    let mut url =
        reqwest::Url::parse(&config.auth_url).map_err(|_error| OAuthError::InvalidConfig)?;
    {
        let mut query = url.query_pairs_mut();
        query.extend_pairs([
            ("response_type", "code"),
            ("client_id", &config.client_id),
            ("redirect_uri", &config.redirect_uri),
            ("state", state),
            ("code_challenge", &pkce.challenge),
            ("code_challenge_method", "S256"),
        ]);
        if !config.scopes.is_empty() {
            query.append_pair("scope", &config.scopes.join(" "));
        }
        if config.is_codex() {
            query.extend_pairs([
                ("id_token_add_organizations", "true"),
                ("codex_cli_simplified_flow", "true"),
                ("originator", "ctx"),
            ]);
        }
    }
    Ok(String::from(url).replace('+', "%20"))
}

pub fn oauth_authorization_code_form(
    config: &OAuthProviderConfig,
    code: &str,
    pkce: &OAuthPkce,
) -> Result<String, OAuthError> {
    if !valid_config(config) || code.is_empty() || controls(code) {
        return Err(OAuthError::InvalidConfig);
    }
    Ok(form(&[
        ("grant_type", "authorization_code"),
        ("client_id", &config.client_id),
        ("code", code),
        ("redirect_uri", &config.redirect_uri),
        ("code_verifier", &pkce.verifier),
    ]))
}

pub fn oauth_refresh_token_form(
    config: &OAuthProviderConfig,
    refresh: &str,
) -> Result<String, OAuthError> {
    if !valid_config(config) || refresh.trim().is_empty() || controls(refresh) {
        return Err(OAuthError::InvalidConfig);
    }
    Ok(form(&[
        ("grant_type", "refresh_token"),
        ("client_id", &config.client_id),
        ("refresh_token", refresh),
    ]))
}

pub fn parse_oauth_token_response(body: &[u8]) -> Result<OAuthTokenResponse, OAuthError> {
    let token: OAuthTokenResponse =
        serde_json::from_slice(body).map_err(|_error| OAuthError::InvalidToken)?;
    if token.access_token.trim().is_empty()
        || token
            .token_type
            .as_deref()
            .is_some_and(|kind| !kind.eq_ignore_ascii_case("bearer"))
    {
        return Err(OAuthError::InvalidToken);
    }
    Ok(token)
}

pub fn oauth_post(
    url: &str,
    content_type: &str,
    body: &str,
    timeout_secs: u64,
) -> Result<(u16, Vec<u8>), OAuthError> {
    if url.is_empty() || controls(url) || controls(content_type) {
        return Err(OAuthError::InvalidConfig);
    }
    let response = reqwest::blocking::Client::new()
        .post(url)
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .header(reqwest::header::CONTENT_TYPE, content_type)
        .body(body.to_owned())
        .send()
        .map_err(|_error| OAuthError::Transport)?;
    let status = response.status().as_u16();
    let output = crate::support::process::read_limited_bytes(response, 1024 * 1024 + 1);
    if output.len() > 1024 * 1024 {
        Err(OAuthError::Transport)
    } else {
        Ok((status, output))
    }
}

pub fn exchange_oauth_token(
    config: &OAuthProviderConfig,
    body: &str,
) -> Result<OAuthTokenResponse, OAuthError> {
    exchange_oauth_token_with(config, body, |url, body| {
        oauth_post(url, "application/x-www-form-urlencoded", body, 30)
    })
}

pub fn exchange_oauth_token_with(
    config: &OAuthProviderConfig,
    body: &str,
    post: impl FnOnce(&str, &str) -> Result<(u16, Vec<u8>), OAuthError>,
) -> Result<OAuthTokenResponse, OAuthError> {
    let (status, body) = post(&config.token_url, body)?;
    if (200..300).contains(&status) {
        parse_oauth_token_response(&body)
    } else {
        Err(OAuthError::Transport)
    }
}

pub fn request_device_code_with(
    mut post: impl FnMut(&str, &str) -> Result<(u16, Vec<u8>), OAuthError>,
) -> Result<DeviceCode, OAuthError> {
    let (status, body) = post(
        CODEX_DEVICE_USER_URL,
        &serde_json::json!({"client_id": CODEX_CLIENT_ID}).to_string(),
    )?;
    if !(200..300).contains(&status) {
        return Err(OAuthError::Transport);
    }
    let device: DeviceCode =
        serde_json::from_slice(&body).map_err(|_error| OAuthError::InvalidToken)?;
    if [&device.id, &device.code]
        .into_iter()
        .any(|value| value.is_empty() || controls(value))
    {
        Err(OAuthError::InvalidToken)
    } else {
        Ok(device)
    }
}

pub fn poll_device_code_with(
    device: &DeviceCode,
    timeout: u64,
    mut post: impl FnMut(&str, &str) -> Result<(u16, Vec<u8>), OAuthError>,
    exchange: impl FnOnce(&str, &str) -> Result<(u16, Vec<u8>), OAuthError>,
    mut pause: impl FnMut(u64),
) -> Result<OAuthTokenResponse, OAuthError> {
    let mut interval = device
        .interval
        .parse::<u64>()
        .map_err(|_error| OAuthError::InvalidToken)?
        .clamp(1, 60);
    let mut remaining = timeout;
    loop {
        let request =
            serde_json::json!({"device_auth_id": device.id, "user_code": device.code}).to_string();
        let (status, body) = post(CODEX_DEVICE_TOKEN_URL, &request)?;
        if (200..300).contains(&status) {
            let grant: DeviceGrant =
                serde_json::from_slice(&body).map_err(|_error| OAuthError::InvalidToken)?;
            let pkce = OAuthPkce::from_verifier(&grant.code_verifier)?;
            if grant
                .code_challenge
                .is_some_and(|value| value != pkce.challenge())
            {
                return Err(OAuthError::InvalidToken);
            }
            let mut config = codex_oauth_config();
            CODEX_DEVICE_REDIRECT_URI.clone_into(&mut config.redirect_uri);
            let form = oauth_authorization_code_form(&config, &grant.authorization_code, &pkce)?;
            return exchange_oauth_token_with(&config, &form, exchange);
        }
        let error = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|value| value.get("error")?.as_str().map(str::to_owned));
        if status == 429 || error.as_deref() == Some("slow_down") {
            interval = (interval + 5).min(60);
        } else if !matches!(status, 403 | 404) && error.as_deref() != Some("authorization_pending")
        {
            return Err(OAuthError::Transport);
        }
        if interval > remaining {
            return Err(OAuthError::Transport);
        }
        pause(interval);
        remaining -= interval;
    }
}

pub fn oauth_token_state(
    token: &OAuthTokenResponse,
    retained: Option<&OAuthTokenState>,
    now: u64,
) -> Result<OAuthTokenState, OAuthError> {
    let access = token.access_token.trim();
    if access.is_empty() {
        return Err(OAuthError::InvalidToken);
    }
    let refresh = token
        .refresh_token
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| retained.map(|value| value.refresh_token.as_str()))
        .ok_or(OAuthError::InvalidToken)?;
    let account = token
        .id_token
        .as_deref()
        .and_then(oauth_account_id)
        .or_else(|| oauth_account_id(&token.access_token))
        .or_else(|| retained.map(|value| value.account_id.clone()))
        .ok_or(OAuthError::InvalidToken)?;
    let expires = token
        .expires_in
        .and_then(|value| now.checked_add(value))
        .or_else(|| jwt(&token.access_token)?.get("exp")?.as_u64())
        .or_else(|| retained.map(|value| value.expires_at))
        .ok_or(OAuthError::InvalidToken)?;
    Ok(OAuthTokenState {
        access_token: access.to_owned(),
        refresh_token: refresh.to_owned(),
        account_id: account,
        expires_at: expires,
    })
}

fn user_slots(config: &OAuthProviderConfig) -> [&str; 4] {
    [
        config.access_account(),
        config.refresh_account(),
        "oauth:account",
        "oauth:expires-at",
    ]
}

fn write_state(
    slots: [&str; 4],
    state: &OAuthTokenState,
    mut write: impl FnMut(&str, &str) -> Result<(), OAuthError>,
) -> Result<(), OAuthError> {
    write(slots[0], &state.access_token)?;
    write(slots[1], &state.refresh_token)?;
    write(slots[2], &state.account_id)?;
    write(slots[3], &state.expires_at.to_string())
}

pub fn store_codex_with(
    state: &OAuthTokenState,
    host: impl FnMut(&str, &str) -> Result<(), OAuthError>,
) -> Result<(), OAuthError> {
    write_state(CODEX_SYSTEM_SLOTS, state, host)
}

pub fn store_codex_system(state: &OAuthTokenState) -> Result<(), OAuthError> {
    store_codex_with(state, |slot, value| {
        crate::provider::name::store_provider_system_secret("codex", slot, value)
            .map_err(|_error| OAuthError::SystemStoreUnavailable)
    })
}

fn read_state(
    slots: [&str; 4],
    mut read: impl FnMut(&str) -> Result<Option<String>, OAuthError>,
) -> Result<Option<OAuthTokenState>, OAuthError> {
    let Some(access_token) = read(slots[0])? else {
        return Ok(None);
    };
    let refresh_token = read(slots[1])?.ok_or(OAuthError::InvalidToken)?;
    let account_id = read(slots[2])?.ok_or(OAuthError::InvalidToken)?;
    let expires = read(slots[3])?.ok_or(OAuthError::InvalidToken)?;
    Ok(Some(OAuthTokenState {
        access_token,
        refresh_token,
        account_id,
        expires_at: expires.parse().map_err(|_error| OAuthError::InvalidToken)?,
    }))
}

pub fn read_codex_system() -> Result<Option<OAuthTokenState>, OAuthError> {
    read_state(CODEX_SYSTEM_SLOTS, |slot| {
        crate::provider::name::read_provider_system_secret("codex", slot)
            .map_err(|_error| OAuthError::SystemStoreUnavailable)
    })
}

pub fn store_oauth_tokens(
    provider: &str,
    config: &OAuthProviderConfig,
    token: &OAuthTokenResponse,
    now: u64,
) -> Result<(), OAuthError> {
    let service = crate::provider::name::provider_keychain_service(provider);
    if config.is_codex() {
        let retained = read_state(user_slots(config), |slot| {
            oauth_keychain_secret(&service, slot)
        })?;
        return write_state(
            user_slots(config),
            &oauth_token_state(token, retained.as_ref(), now)?,
            |slot, value| oauth_keychain_set(&service, slot, value),
        );
    }
    oauth_keychain_set(&service, config.access_account(), &token.access_token)?;
    if let Some(refresh) = token
        .refresh_token
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        oauth_keychain_set(&service, config.refresh_account(), refresh)?;
    }
    if let Some(expires_at) = token.expires_in.and_then(|value| now.checked_add(value)) {
        oauth_keychain_set(&service, OAUTH_EXPIRES_ACCOUNT, &expires_at.to_string())?;
    }
    Ok(())
}

pub fn resolve_codex_with(
    config: &OAuthProviderConfig,
    stored: &mut Option<OAuthTokenState>,
    now: u64,
    exchange: impl FnOnce(&str) -> Result<OAuthTokenResponse, OAuthError>,
) -> Result<Option<OAuthCredential>, OAuthError> {
    if !config.is_codex() || !valid_config(config) {
        return Err(OAuthError::InvalidConfig);
    }
    let Some(state) = stored.as_mut() else {
        return Ok(None);
    };
    if state.access_token.trim().is_empty() || state.account_id.trim().is_empty() {
        return Err(OAuthError::InvalidToken);
    }
    if oauth_needs_refresh(state.expires_at, now) {
        if state.refresh_token.trim().is_empty() {
            return Err(OAuthError::InvalidToken);
        }
        *state = oauth_token_state(
            &exchange(&oauth_refresh_token_form(config, &state.refresh_token)?)?,
            Some(state),
            now,
        )?;
    }
    Ok(Some((state.access_token.clone(), state.account_id.clone())))
}

pub fn resolve_codex_system() -> Result<Option<OAuthCredential>, OAuthError> {
    let config = codex_oauth_config();
    resolve_stored_codex(&config, read_codex_system()?, store_codex_system)
}

pub fn resolve_oauth_credential(
    provider: &str,
    config: &OAuthProviderConfig,
) -> Result<Option<OAuthCredential>, OAuthError> {
    if !config.is_codex() {
        return resolve_generic_oauth(provider, config);
    }
    let service = crate::provider::name::provider_keychain_service(provider);
    let slots = user_slots(config);
    let stored = read_state(slots, |slot| oauth_keychain_secret(&service, slot))?;
    resolve_stored_codex(config, stored, |state| {
        write_state(slots, state, |slot, value| {
            oauth_keychain_set(&service, slot, value)
        })
    })
}

fn resolve_generic_oauth(
    provider: &str,
    config: &OAuthProviderConfig,
) -> Result<Option<OAuthCredential>, OAuthError> {
    if provider.is_empty() || controls(provider) || !valid_config(config) {
        return Err(OAuthError::InvalidConfig);
    }
    let access = crate::provider_oauth_access_token_env_name(provider);
    match env::var(&access) {
        Ok(value) if !value.trim().is_empty() => return Ok(Some((value, String::new()))),
        Ok(_) | Err(env::VarError::NotPresent) => {}
        Err(env::VarError::NotUnicode(_)) => return Err(OAuthError::InvalidConfig),
    }
    let service = crate::provider::name::provider_keychain_service(provider);
    let access = oauth_keychain_secret(&service, config.access_account())?;
    let refresh = oauth_keychain_secret(&service, config.refresh_account())?;
    let expires = oauth_keychain_secret(&service, OAUTH_EXPIRES_ACCOUNT)?
        .and_then(|value| value.parse::<u64>().ok());
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_error| OAuthError::Transport)?
        .as_secs();
    let stale = expires.is_some_and(|value| oauth_needs_refresh(value, now));
    if !stale && let Some(access) = access.as_ref() {
        return Ok(Some((access.clone(), String::new())));
    }
    let Some(refresh) = refresh.filter(|value| !value.trim().is_empty()) else {
        return Ok(access.map(|value| (value, String::new())));
    };
    let form = oauth_refresh_token_form(config, &refresh)?;
    let token = exchange_oauth_token(config, &form)?;
    store_oauth_tokens(provider, config, &token, now)?;
    Ok(Some((token.access_token, String::new())))
}

fn resolve_stored_codex(
    config: &OAuthProviderConfig,
    mut stored: Option<OAuthTokenState>,
    store: impl FnOnce(&OAuthTokenState) -> Result<(), OAuthError>,
) -> Result<Option<OAuthCredential>, OAuthError> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_error| OAuthError::Transport)?
        .as_secs();
    let credential = resolve_codex_with(config, &mut stored, now, |form| {
        exchange_oauth_token(config, form)
    })?;
    if let Some(state) = stored {
        store(&state)?;
    }
    Ok(credential)
}

#[must_use]
pub fn oauth_needs_refresh(expires_at: u64, now: u64) -> bool {
    expires_at <= now.saturating_add(300)
}

pub fn oauth_account_id(token: &str) -> Option<String> {
    let value = jwt(token)?;
    value
        .get("chatgpt_account_id")
        .or_else(|| value.pointer("/https:~1~1api.openai.com~1auth/chatgpt_account_id"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
}

pub fn resolve_oauth_access_token(
    provider: &str,
    config: &OAuthProviderConfig,
) -> Result<Option<String>, OAuthError> {
    if !config.is_codex() {
        return resolve_generic_oauth(provider, config)
            .map(|value| value.map(|(token, _account)| token));
    }
    resolve_oauth_access_token_with(
        provider,
        config,
        |name| env::var(name),
        oauth_keychain_secret,
    )
}

pub fn resolve_oauth_access_token_with(
    provider: &str,
    config: &OAuthProviderConfig,
    env_lookup: impl Fn(&str) -> Result<String, env::VarError>,
    keychain_lookup: impl FnOnce(&str, &str) -> Result<Option<String>, OAuthError>,
) -> Result<Option<String>, OAuthError> {
    if provider.is_empty() || controls(provider) || !valid_config(config) {
        return Err(OAuthError::InvalidConfig);
    }
    let name = crate::provider_oauth_access_token_env_name(provider);
    match env_lookup(&name) {
        Ok(value) if !value.trim().is_empty() => Ok(Some(value)),
        Ok(_) | Err(env::VarError::NotPresent) => keychain_lookup(
            &crate::provider::name::provider_keychain_service(provider),
            config.access_account(),
        ),
        Err(env::VarError::NotUnicode(_)) => Err(OAuthError::InvalidConfig),
    }
}

pub fn oauth_keychain_secret(service: &str, account: &str) -> Result<Option<String>, OAuthError> {
    let entry = match keyring::Entry::new(service, account) {
        Ok(entry) => entry,
        Err(keyring::Error::NoDefaultStore) => return Ok(None),
        Err(_) => return Err(OAuthError::KeychainUnavailable),
    };
    match entry.get_password() {
        Ok(secret) if !secret.is_empty() => Ok(Some(secret)),
        Ok(_) | Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err(OAuthError::KeychainUnavailable),
    }
}

fn oauth_keychain_set(service: &str, account: &str, secret: &str) -> Result<(), OAuthError> {
    keyring::Entry::new(service, account)
        .and_then(|entry| entry.set_password(secret))
        .map_err(|_error| OAuthError::KeychainUnavailable)
}

fn valid_config(config: &OAuthProviderConfig) -> bool {
    [
        &config.client_id,
        &config.auth_url,
        &config.token_url,
        &config.redirect_uri,
    ]
    .into_iter()
    .all(|value| !value.is_empty() && !controls(value))
        && !config
            .scopes
            .iter()
            .any(|scope| scope.trim().is_empty() || controls(scope))
}

fn controls(value: &str) -> bool {
    value.bytes().any(|byte| byte.is_ascii_control())
}

fn jwt(token: &str) -> Option<serde_json::Value> {
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(token.split('.').nth(1)?)
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn form(pairs: &[(&str, &str)]) -> String {
    let Ok(mut url) = reqwest::Url::parse("http://localhost") else {
        return String::new();
    };
    url.query_pairs_mut().extend_pairs(pairs.iter().copied());
    url.query().unwrap_or_default().replace('+', "%20")
}
