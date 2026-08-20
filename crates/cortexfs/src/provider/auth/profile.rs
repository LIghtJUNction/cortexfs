use serde::{Deserialize, Serialize};

use super::Credential;

/// One atomically stored authentication profile for a provider.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthProfile {
    pub revision: u64,
    pub credential: Credential,
}

/// Stable failures while persisting an authentication profile.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AuthProfileError {
    #[error("invalid authentication profile")]
    Invalid,
    #[error("authentication profile store is unavailable")]
    Unavailable,
}

impl AuthProfile {
    #[must_use]
    pub const fn credential(&self) -> &Credential {
        &self.credential
    }
}

/// Reads one provider profile from the root-owned `CortexFS` secret store.
pub fn read_auth_profile(
    provider: &str,
    profile: &str,
) -> Result<Option<AuthProfile>, AuthProfileError> {
    let account = account(profile)?;
    let Some(raw) = crate::provider::name::read_provider_system_secret(provider, &account)
        .map_err(|_error| AuthProfileError::Unavailable)?
    else {
        return Ok(None);
    };
    let value =
        serde_json::from_str::<AuthProfile>(&raw).map_err(|_error| AuthProfileError::Invalid)?;
    (value.credential.provider() == provider)
        .then_some(value)
        .ok_or(AuthProfileError::Invalid)
        .map(Some)
}

/// Atomically writes a complete provider profile and advances its revision.
pub fn store_auth_profile(
    provider: &str,
    profile: &str,
    credential: Credential,
) -> Result<AuthProfile, AuthProfileError> {
    if credential.provider() != provider {
        return Err(AuthProfileError::Invalid);
    }
    let account = account(profile)?;
    let revision = read_auth_profile(provider, profile)?
        .map_or(1, |current| current.revision.saturating_add(1));
    let value = AuthProfile {
        revision,
        credential,
    };
    let encoded = serde_json::to_string(&value).map_err(|_error| AuthProfileError::Unavailable)?;
    crate::provider::name::store_provider_system_secret(provider, &account, &encoded)
        .map_err(|_error| AuthProfileError::Unavailable)?;
    Ok(value)
}

fn account(profile: &str) -> Result<String, AuthProfileError> {
    crate::is_object_name(profile)
        .then(|| format!("auth-{profile}"))
        .ok_or(AuthProfileError::Invalid)
}

#[cfg(test)]
mod tests;
