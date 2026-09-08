use super::{AuthProviderError, AuthResponse, Credential};
use crate::provider::oauth::{OAuthTokenResponse, parse_oauth_token_response};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn unix_time() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

pub fn credential_from_token(
    provider: &str,
    token: OAuthTokenResponse,
    retained: Option<&Credential>,
    now: u64,
) -> Result<Credential, AuthProviderError> {
    let access_token = token.access_token.trim();
    if access_token.is_empty() {
        return Err(AuthProviderError::InvalidResponse);
    }
    if retained.is_some_and(|credential| credential.provider() != provider) {
        return Err(AuthProviderError::InvalidCredential);
    }
    let (old_refresh, old_scopes) =
        retained.map_or_else(Default::default, |credential| match *credential {
            Credential::OAuth {
                ref refresh_token,
                ref scopes,
                ..
            } => (refresh_token.clone(), scopes.clone()),
            Credential::ApiKey { .. } => Default::default(),
        });
    let refresh_token = token
        .refresh_token
        .or(old_refresh)
        .filter(|value| !value.trim().is_empty());
    let expires_at = token
        .expires_in
        .map(|value| {
            now.checked_add(value)
                .ok_or(AuthProviderError::InvalidResponse)
        })
        .transpose()?;
    let scopes = token.scope.map_or(old_scopes, |scope| {
        scope.split_whitespace().map(str::to_owned).collect()
    });
    Ok(Credential::OAuth {
        provider: provider.to_owned(),
        access_token: access_token.to_owned(),
        refresh_token,
        expires_at,
        scopes,
    })
}

pub fn parse_token(response: &AuthResponse) -> Result<OAuthTokenResponse, AuthProviderError> {
    if !(200..300).contains(&response.status) {
        return Err(AuthProviderError::Unavailable);
    }
    parse_oauth_token_response(&response.body).map_err(|_error| AuthProviderError::InvalidResponse)
}

pub fn parse_models(response: &AuthResponse) -> Result<Vec<String>, AuthProviderError> {
    if !(200..300).contains(&response.status) {
        return Err(AuthProviderError::Unavailable);
    }
    let value: serde_json::Value = serde_json::from_slice(&response.body)
        .map_err(|_error| AuthProviderError::InvalidResponse)?;
    let values = value
        .get("data")
        .or_else(|| value.get("models"))
        .and_then(serde_json::Value::as_array)
        .ok_or(AuthProviderError::InvalidResponse)?;
    let names = values
        .iter()
        .filter_map(|item| item.get("id").and_then(serde_json::Value::as_str))
        .map(str::to_owned);
    Ok(crate::provider::discovery::provider_model_names(names))
}
