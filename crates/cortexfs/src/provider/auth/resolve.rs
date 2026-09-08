use super::{AuthMethod, AuthProvider, AuthProviderError, Credential, resolve_auth_profile};
use crate::provider::oauth::{OAuthProviderConfig, resolve_oauth_credential_with};

/// Keeps the profile and legacy credential precedence identical at every host boundary.
pub fn resolve_credential(
    adapter: &dyn AuthProvider,
    oauth: Option<&OAuthProviderConfig>,
    profile: &str,
) -> Result<Option<Credential>, AuthProviderError> {
    let mut denied = false;
    match resolve_auth_profile(adapter, profile, false) {
        Ok(Some(credential)) => return Ok(Some(credential)),
        Ok(None) => {}
        Err(AuthProviderError::StoreAccessDenied) => denied = true,
        Err(error) => return Err(error),
    }
    let provider = adapter.id();
    if let Some(method) = adapter
        .methods()
        .iter()
        .find(|method| method.method == AuthMethod::ApiKey)
    {
        let slot = if profile == "default" {
            method.slot.as_str()
        } else {
            profile
        };
        if !super::is_api_key_slot(slot) {
            return Err(AuthProviderError::InvalidCredential);
        }
        match crate::read_provider_system_secret(provider, slot) {
            Ok(Some(key)) => {
                let credential = Credential::ApiKey {
                    provider: provider.to_owned(),
                    key,
                    slot: Some(slot.to_owned()),
                };
                adapter.model_headers(&credential)?;
                return Ok(Some(credential));
            }
            Ok(None) => {}
            Err(crate::ProviderSystemSecretError::AccessDenied) => denied = true,
            Err(_error) => return Err(AuthProviderError::Unavailable),
        }
    }
    let missing = if denied {
        Err(AuthProviderError::StoreAccessDenied)
    } else {
        Ok(None)
    };
    if profile != "default"
        || !adapter
            .methods()
            .iter()
            .any(|method| method.method == AuthMethod::OAuth)
    {
        return missing;
    }
    let Some(oauth) = oauth else { return missing };
    let legacy = if provider == "codex" && oauth.is_codex() && !denied {
        crate::resolve_codex_system().map_err(|_error| AuthProviderError::Unavailable)?
    } else {
        None
    };
    let token = match legacy {
        Some(token) => Some(token),
        None => resolve_oauth_credential_with(provider, oauth, |request| {
            super::refresh_oauth_result(provider, request, adapter)
        })
        .map_err(|_error| AuthProviderError::Unavailable)?,
    };
    let Some((access_token, _account)) = token else {
        return missing;
    };
    Ok(Some(Credential::OAuth {
        provider: provider.to_owned(),
        access_token,
        refresh_token: None,
        expires_at: None,
        scopes: Vec::new(),
    }))
}
