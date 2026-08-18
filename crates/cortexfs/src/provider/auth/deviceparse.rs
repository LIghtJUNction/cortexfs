use super::device::DeviceChallenge;
use super::{AuthProviderError, AuthResponse};
use serde::Deserialize;

#[derive(Deserialize)]
struct DeviceResponse {
    device_code: String,
    user_code: String,
    verification_uri: Option<String>,
    expires_in: u64,
    interval: Option<u64>,
}

#[derive(Deserialize)]
struct DeviceError {
    error: Option<String>,
}

pub(super) fn parse_challenge(
    response: &AuthResponse,
    fallback: &str,
) -> Result<(DeviceChallenge, String), AuthProviderError> {
    if !(200..300).contains(&response.status) {
        return Err(AuthProviderError::Unavailable);
    }
    let value: DeviceResponse = serde_json::from_slice(&response.body)
        .map_err(|_error| AuthProviderError::InvalidResponse)?;
    let uri = value
        .verification_uri
        .unwrap_or_else(|| fallback.to_owned());
    if value.device_code.trim().is_empty()
        || value.user_code.trim().is_empty()
        || uri.trim().is_empty()
        || value.expires_in == 0
        || controls(&value.device_code)
        || controls(&value.user_code)
        || controls(&uri)
    {
        return Err(AuthProviderError::InvalidResponse);
    }
    Ok((
        DeviceChallenge {
            verification_uri: uri,
            user_code: value.user_code,
            expires_in: value.expires_in,
            interval: value.interval.unwrap_or(5),
        },
        value.device_code,
    ))
}

fn controls(value: &str) -> bool {
    value.bytes().any(|byte| byte.is_ascii_control())
}

pub(super) fn parse_error(response: &AuthResponse) -> Option<String> {
    serde_json::from_slice::<DeviceError>(&response.body)
        .ok()
        .and_then(|value| value.error)
}

pub(super) fn form(pairs: &[(&str, &str)]) -> String {
    let Ok(mut url) = reqwest::Url::parse("http://localhost") else {
        return String::new();
    };
    url.query_pairs_mut().extend_pairs(pairs.iter().copied());
    url.query().unwrap_or_default().replace('+', "%20")
}
