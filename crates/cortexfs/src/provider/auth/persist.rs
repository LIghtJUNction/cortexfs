use super::common::AdapterCore;
use super::{AuthMethod, AuthProviderError, Credential};

pub fn store(
    core: &AdapterCore,
    credential: &Credential,
    now: u64,
) -> Result<(), AuthProviderError> {
    if credential.provider() != core.id {
        return Err(AuthProviderError::InvalidCredential);
    }
    match *credential {
        Credential::ApiKey {
            ref key, ref slot, ..
        } => store_key(core, key, slot.as_deref()),
        Credential::OAuth { .. } => {
            if !core
                .methods
                .iter()
                .any(|method| method.method == AuthMethod::OAuth)
            {
                return Err(AuthProviderError::UnsupportedMethod);
            }
            let _ = now;
            super::profile::store_auth_profile(&core.id, "default", credential.clone())
                .map(|_profile| ())
                .map_err(|_error| AuthProviderError::Unavailable)
        }
    }
}

fn store_key(
    core: &AdapterCore,
    key: &str,
    requested_slot: Option<&str>,
) -> Result<(), AuthProviderError> {
    if key.trim().is_empty() || key.bytes().any(|byte| byte.is_ascii_control()) {
        return Err(AuthProviderError::InvalidCredential);
    }
    let slot = requested_slot
        .or_else(|| {
            core.methods
                .iter()
                .find(|method| method.method == AuthMethod::ApiKey)
                .map(|method| method.slot.as_str())
        })
        .ok_or(AuthProviderError::UnsupportedMethod)?;
    if !core
        .methods
        .iter()
        .any(|method| method.method == AuthMethod::ApiKey && method.slot == slot)
    {
        return Err(AuthProviderError::InvalidCredential);
    }
    crate::provider::name::store_provider_system_secret(&core.id, slot, key)
        .map_err(|_error| AuthProviderError::Unavailable)
}
