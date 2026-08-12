use super::deviceparse::{form, parse_challenge, parse_error};
use super::{AuthProviderError, AuthTransport};
use crate::provider::oauth::{OAuthDeviceConfig, OAuthProviderConfig, parse_oauth_token_response};

/// User-visible challenge emitted by a device authorization flow.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceChallenge {
    pub verification_uri: String,
    pub user_code: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// Provider-specific endpoints for the standard device authorization grant.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeviceConfig {
    pub request_url: String,
    pub token_url: String,
    pub verification_uri: String,
}

impl From<OAuthDeviceConfig> for DeviceConfig {
    fn from(config: OAuthDeviceConfig) -> Self {
        Self {
            request_url: config.request_url,
            token_url: config.token_url,
            verification_uri: config.verification_uri,
        }
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "device flow injects transport, clock, notifier, and sleep for hermetic hosts"
)]
pub fn login(
    provider: &str,
    oauth: &OAuthProviderConfig,
    device: &DeviceConfig,
    timeout_secs: u64,
    transport: &mut dyn AuthTransport,
    now: u64,
    notify: &mut dyn FnMut(&DeviceChallenge),
    pause: &mut dyn FnMut(u64),
) -> Result<super::Credential, AuthProviderError> {
    if oauth.client_id.trim().is_empty()
        || timeout_secs == 0
        || !oauth
            .device
            .as_ref()
            .is_none_or(OAuthDeviceConfig::is_valid)
        || [
            &device.request_url,
            &device.token_url,
            &device.verification_uri,
        ]
        .into_iter()
        .any(|value| value.is_empty() || value.bytes().any(|byte| byte.is_ascii_control()))
    {
        return Err(AuthProviderError::InvalidConfig);
    }
    let response = transport.post_with_headers(
        &device.request_url,
        "application/x-www-form-urlencoded",
        &form(&[
            ("client_id", &oauth.client_id),
            ("scope", &oauth.scopes.join(" ")),
        ]),
        &[("Accept", "application/json")],
    )?;
    let (challenge, device_code) = parse_challenge(&response, &device.verification_uri)?;
    notify(&challenge);
    let mut interval = challenge.interval.clamp(1, 60);
    let mut remaining = timeout_secs.min(challenge.expires_in);
    loop {
        let body = form(&[
            ("client_id", &oauth.client_id),
            ("device_code", &device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ]);
        let response = transport.post_with_headers(
            &device.token_url,
            "application/x-www-form-urlencoded",
            &body,
            &[("Accept", "application/json")],
        )?;
        if (200..300).contains(&response.status) {
            let token = parse_oauth_token_response(&response.body)
                .map_err(|_error| AuthProviderError::InvalidResponse)?;
            return super::protocol::credential_from_token(provider, token, None, now);
        }
        let error = parse_error(&response);
        match error.as_deref() {
            Some("authorization_pending") => {}
            Some("slow_down") => interval = (interval + 5).min(60),
            Some("expired_token" | "access_denied") => {
                return Err(AuthProviderError::InvalidCredential);
            }
            _ => return Err(AuthProviderError::Unavailable),
        }
        if interval > remaining {
            return Err(AuthProviderError::Unavailable);
        }
        pause(interval);
        remaining -= interval;
    }
}
