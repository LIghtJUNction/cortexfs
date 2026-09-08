use serde::{Deserialize, Serialize};

use super::Credential;
use nix::fcntl::{Flock, FlockArg};
use std::fs::File;

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
    #[error("authentication profile store access denied")]
    AccessDenied,
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
        .map_err(|error| match error {
            crate::ProviderSystemSecretError::AccessDenied => AuthProfileError::AccessDenied,
            crate::ProviderSystemSecretError::InvalidName => AuthProfileError::Invalid,
            _ => AuthProfileError::Unavailable,
        })?
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
    let path = crate::provider::name::selection::provider_system_secret_path(provider, &account)
        .map_err(|_error| AuthProfileError::Invalid)?;
    let parent = path.parent().ok_or(AuthProfileError::Invalid)?;
    crate::provider::name::files::create_private_provider_secret_dir(parent)
        .map_err(|_error| AuthProfileError::Unavailable)?;
    let _lock = lock(provider)?;
    write(provider, profile, credential)
}

pub(super) fn write(
    provider: &str,
    profile: &str,
    credential: Credential,
) -> Result<AuthProfile, AuthProfileError> {
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

/// Deletes one stored authentication profile; environment credentials remain untouched.
pub fn delete_auth_profile(provider: &str, profile: &str) -> Result<(), AuthProfileError> {
    let account = account(profile)?;
    if !crate::provider_system_secret_exists(provider, &account)
        .map_err(|_error| AuthProfileError::Unavailable)?
    {
        return Ok(());
    }
    let _lock = lock(provider)?;
    crate::provider::name::delete_provider_system_secret(provider, &account)
        .map_err(|_error| AuthProfileError::Unavailable)
}

pub(super) fn lock(provider: &str) -> Result<Flock<File>, AuthProfileError> {
    let path = crate::provider::name::selection::provider_system_secret_path(provider, "default")
        .map_err(|_error| AuthProfileError::Invalid)?;
    let directory = crate::support::plain::open_plain_directory(
        path.parent().ok_or(AuthProfileError::Invalid)?,
    )
    .map_err(|_error| AuthProfileError::Unavailable)?;
    Flock::lock(directory, FlockArg::LockExclusive)
        .map_err(|(_directory, _error)| AuthProfileError::Unavailable)
}

fn account(profile: &str) -> Result<String, AuthProfileError> {
    crate::is_object_name(profile)
        .then(|| format!("auth-{profile}"))
        .ok_or(AuthProfileError::Invalid)
}

#[cfg(test)]
mod tests;
