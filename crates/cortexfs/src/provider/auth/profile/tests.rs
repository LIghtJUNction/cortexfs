use super::*;

#[test]
fn profile_rejects_unsafe_name_before_secret_store_access() {
    let credential = Credential::ApiKey {
        provider: "local".to_owned(),
        key: "secret".to_owned(),
        slot: None,
    };
    assert_eq!(
        store_auth_profile("local", "../unsafe", credential),
        Err(AuthProfileError::Invalid)
    );
}

#[test]
fn profile_rejects_credential_for_another_provider() {
    let credential = Credential::ApiKey {
        provider: "other".to_owned(),
        key: "secret".to_owned(),
        slot: None,
    };
    assert_eq!(
        store_auth_profile("local", "default", credential),
        Err(AuthProfileError::Invalid)
    );
}
