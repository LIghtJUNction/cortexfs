use crate::{
    AuthMethod, Credential, CredentialKind, OAuthFlow, ProviderAuthConfig, effective_auth_methods,
};

#[test]
fn explicit_auth_methods_replace_legacy_defaults() {
    let methods = vec![ProviderAuthConfig::oauth(
        OAuthFlow::DeviceCode,
        "subscription",
    )];
    let effective = effective_auth_methods(&methods, true);
    assert_eq!(effective, methods);
}

#[test]
fn legacy_oauth_keeps_api_key_and_oauth_compatibility() {
    let methods = effective_auth_methods(&[], true);
    assert!(matches!(methods.first(), Some(method) if method.method == AuthMethod::ApiKey));
    assert!(
        matches!(methods.get(1), Some(method) if method.flow == Some(OAuthFlow::AuthorizationCode))
    );
}

#[test]
fn credential_envelope_round_trips_without_provider_specific_shape()
-> Result<(), Box<dyn std::error::Error>> {
    let credential = Credential::OAuth {
        provider: "openai".to_owned(),
        access_token: "access".to_owned(),
        refresh_token: Some("refresh".to_owned()),
        expires_at: Some(100),
        scopes: vec!["model.read".to_owned()],
    };
    let json = serde_json::to_string(&credential)?;
    let restored: Credential = serde_json::from_str(&json)?;
    assert_eq!(restored, credential);
    assert_eq!(restored.kind(), CredentialKind::OAuth);
    assert!(restored.is_expired(100));
    Ok(())
}

#[test]
fn auth_config_rejects_unsafe_slot_names() {
    let config = ProviderAuthConfig::api_key("../secret");
    assert!(!config.is_valid());
}
