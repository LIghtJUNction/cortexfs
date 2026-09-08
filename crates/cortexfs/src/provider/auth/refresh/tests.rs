use super::usable;
use crate::provider::auth::{AuthProviderError, Credential};
use std::cell::Cell;

#[test]
fn refresh_checks_expiry_identity_and_result_before_use() {
    let refreshes = Cell::new(0);
    let unavailable = |_: &Credential| {
        refreshes.set(refreshes.get() + 1);
        Err(AuthProviderError::Unavailable)
    };
    let adapter = super::super::openai::OpenAiAdapter::codex();
    let mut credential = Credential::OAuth {
        provider: "codex".to_owned(),
        access_token: "old".to_owned(),
        refresh_token: None,
        expires_at: Some(100),
        scopes: Vec::new(),
    };
    assert_eq!(
        usable(&credential, &adapter, 99, false, unavailable),
        Ok(credential.clone())
    );
    assert_eq!(
        usable(&credential, &adapter, 100, false, unavailable),
        Err(AuthProviderError::ExpiredCredential)
    );
    if let Credential::OAuth {
        ref mut refresh_token,
        ..
    } = credential
    {
        *refresh_token = Some("refresh".to_owned());
    }
    let mut fresh = credential.clone();
    if let Credential::OAuth {
        ref mut access_token,
        ref mut expires_at,
        ..
    } = fresh
    {
        "fresh".clone_into(access_token);
        *expires_at = Some(1000);
    }
    assert_eq!(
        usable(&credential, &adapter, 100, false, |_| Ok(fresh.clone())),
        Ok(fresh.clone())
    );
    assert_eq!(
        usable(&fresh, &adapter, 100, true, |_| Err(
            AuthProviderError::Unavailable
        )),
        Err(AuthProviderError::Unavailable)
    );
    assert_eq!(
        usable(
            &credential,
            &adapter,
            100,
            false,
            |_| Ok(credential.clone())
        ),
        Err(AuthProviderError::ExpiredCredential)
    );
    if let Credential::OAuth {
        ref mut provider, ..
    } = fresh
    {
        "other".clone_into(provider);
    }
    assert_eq!(
        usable(&credential, &adapter, 100, false, |_| Ok(fresh)),
        Err(AuthProviderError::InvalidCredential)
    );
    let key = Credential::ApiKey {
        provider: "codex".to_owned(),
        key: "key".to_owned(),
        slot: None,
    };
    assert_eq!(
        usable(&key, &adapter, 100, false, unavailable),
        Err(AuthProviderError::UnsupportedMethod)
    );
    assert_eq!(refreshes.get(), 0);
}
