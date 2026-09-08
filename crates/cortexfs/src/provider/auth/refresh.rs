use super::{AuthMethod, AuthProvider, AuthProviderError, Credential, CredentialKind};

/// Resolves a host profile, serializing OAuth refresh with login and logout.
pub fn resolve_auth_profile(
    adapter: &dyn AuthProvider,
    profile: &str,
    force_refresh: bool,
) -> Result<Option<Credential>, AuthProviderError> {
    if super::read_auth_profile(adapter.id(), profile)
        .map_err(|error| match error {
            super::AuthProfileError::AccessDenied => AuthProviderError::StoreAccessDenied,
            error => error.into(),
        })?
        .is_none()
    {
        return Ok(None);
    }
    let _lock = super::profile::lock(adapter.id()).map_err(AuthProviderError::from)?;
    let Some(current) =
        super::read_auth_profile(adapter.id(), profile).map_err(AuthProviderError::from)?
    else {
        return Ok(None);
    };
    let credential = usable(
        current.credential(),
        adapter,
        super::protocol::unix_time(),
        force_refresh,
        |credential| adapter.refresh(credential),
    )?;
    if credential != current.credential {
        super::profile::write(adapter.id(), profile, credential.clone())
            .map_err(AuthProviderError::from)?;
    }
    Ok(Some(credential))
}

impl From<super::AuthProfileError> for AuthProviderError {
    fn from(error: super::AuthProfileError) -> Self {
        match error {
            super::AuthProfileError::Invalid => Self::InvalidCredential,
            super::AuthProfileError::Unavailable | super::AuthProfileError::AccessDenied => {
                Self::Unavailable
            }
        }
    }
}

pub(super) fn usable(
    credential: &Credential,
    adapter: &dyn AuthProvider,
    now: u64,
    force_refresh: bool,
    refresh: impl FnOnce(&Credential) -> Result<Credential, AuthProviderError>,
) -> Result<Credential, AuthProviderError> {
    let method = match credential.kind() {
        CredentialKind::ApiKey => AuthMethod::ApiKey,
        CredentialKind::OAuth => AuthMethod::OAuth,
    };
    if !adapter.methods().iter().any(|entry| entry.method == method) {
        return Err(AuthProviderError::UnsupportedMethod);
    }
    if force_refresh && credential.kind() != CredentialKind::OAuth {
        return Err(AuthProviderError::UnsupportedMethod);
    }
    adapter.model_headers(credential)?;
    let &Credential::OAuth {
        expires_at,
        ref refresh_token,
        ..
    } = credential
    else {
        return Ok(credential.clone());
    };
    if !force_refresh
        && expires_at.is_none_or(|expires_at| !crate::oauth_needs_refresh(expires_at, now))
    {
        return Ok(credential.clone());
    }
    if refresh_token
        .as_ref()
        .is_none_or(|value| value.trim().is_empty())
    {
        if force_refresh {
            return Err(AuthProviderError::InvalidCredential);
        }
        return if credential.is_expired(now) {
            Err(AuthProviderError::ExpiredCredential)
        } else {
            Ok(credential.clone())
        };
    }
    let refreshed = refresh(credential)?;
    if refreshed.kind() != CredentialKind::OAuth {
        return Err(AuthProviderError::InvalidCredential);
    }
    adapter.model_headers(&refreshed)?;
    if refreshed.is_expired(now) {
        return Err(AuthProviderError::ExpiredCredential);
    }
    Ok(refreshed)
}

#[cfg(test)]
mod tests;
