use super::device::DeviceChallenge;
use super::protocol::credential_from_token;
use super::{AuthProviderError, AuthTransport, Credential};
use crate::provider::oauth::{
    CODEX_DEVICE_VERIFY_URL, OAuthError, poll_device_code_with, request_device_code_with,
};
use std::cell::RefCell;

pub fn login(
    provider: &str,
    timeout_secs: u64,
    transport: &mut dyn AuthTransport,
    now: u64,
    notify: &mut dyn FnMut(&DeviceChallenge),
    pause: &mut dyn FnMut(u64),
) -> Result<Credential, AuthProviderError> {
    if timeout_secs == 0 {
        return Err(AuthProviderError::InvalidConfig);
    }
    let transport = RefCell::new(transport);
    let device =
        request_device_code_with(|url, body| post(&transport, url, "application/json", body))
            .map_err(map_error)?;
    notify(&DeviceChallenge {
        verification_uri: CODEX_DEVICE_VERIFY_URL.to_owned(),
        user_code: device.code.clone(),
        expires_in: timeout_secs,
        interval: device.interval.parse().unwrap_or(5),
    });
    let token = poll_device_code_with(
        &device,
        timeout_secs,
        |url, body| post(&transport, url, "application/json", body),
        |url, body| post(&transport, url, "application/x-www-form-urlencoded", body),
        pause,
    )
    .map_err(map_error)?;
    credential_from_token(provider, token, None, now)
}

fn post(
    transport: &RefCell<&mut dyn AuthTransport>,
    url: &str,
    content_type: &str,
    body: &str,
) -> Result<(u16, Vec<u8>), OAuthError> {
    transport
        .borrow_mut()
        .post(url, content_type, body)
        .map(|response| (response.status, response.body))
        .map_err(|_error| OAuthError::Transport)
}

fn map_error(error: OAuthError) -> AuthProviderError {
    match error {
        OAuthError::InvalidConfig | OAuthError::InvalidVerifier => AuthProviderError::InvalidConfig,
        OAuthError::InvalidToken => AuthProviderError::InvalidResponse,
        OAuthError::KeychainUnavailable | OAuthError::SystemStoreUnavailable => {
            AuthProviderError::Unavailable
        }
        OAuthError::Transport => AuthProviderError::Unavailable,
    }
}
