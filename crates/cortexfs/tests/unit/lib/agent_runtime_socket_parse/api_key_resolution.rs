#[test]
fn api_key_resolution_prefers_environment_over_keychain() {
    let resolved = resolve_api_key_with(
        "CTX_LMM_SECRET",
        "cortexfs:lmm",
        "default",
        |_name| Ok("env-secret".to_owned()),
        |_service, _account| Ok(Some("keychain-secret".to_owned())),
    );
    assert_eq!(resolved, Ok(Some("env-secret".to_owned())));
}

#[test]
fn api_key_resolution_uses_keychain_when_environment_is_empty_or_missing() {
    let empty_env = resolve_api_key_with(
        "CTX_LMM_SECRET",
        "cortexfs:lmm",
        "default",
        |_name| Ok(" \n".to_owned()),
        |_service, _account| Ok(Some("keychain-secret".to_owned())),
    );
    assert_eq!(empty_env, Ok(Some("keychain-secret".to_owned())));

    let missing_env = resolve_api_key_with(
        "CTX_LMM_SECRET",
        "cortexfs:lmm",
        "default",
        |_name| Err(std::env::VarError::NotPresent),
        |_service, _account| Ok(Some("keychain-secret".to_owned())),
    );
    assert_eq!(missing_env, Ok(Some("keychain-secret".to_owned())));
}

#[test]
fn api_key_resolution_checks_all_environment_candidates_before_keychain() {
    let env_names = vec![
        "CTX_TEST_PRIMARY_SECRET".to_owned(),
        "CTX_TEST_SECONDARY_SECRET".to_owned(),
    ];
    let resolved = resolve_api_key_from_env_names_with(
        &env_names,
        "cortexfs:test",
        "default",
        |name| {
            if name == "CTX_TEST_SECONDARY_SECRET" {
                Ok("env-secret".to_owned())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        },
        |_service, _account| Ok(Some("keychain-secret".to_owned())),
    );
    assert_eq!(resolved, Ok(Some("env-secret".to_owned())));
}

#[test]
fn api_key_resolution_uses_keychain_after_environment_candidates() {
    let env_names = vec![
        "CTX_TEST_PRIMARY_SECRET".to_owned(),
        "CTX_TEST_SECONDARY_SECRET".to_owned(),
    ];
    let resolved = resolve_api_key_from_env_names_with(
        &env_names,
        "cortexfs:test",
        "default",
        |_name| Err(std::env::VarError::NotPresent),
        |_service, _account| Ok(Some("keychain-secret".to_owned())),
    );
    assert_eq!(resolved, Ok(Some("keychain-secret".to_owned())));
}

#[test]
fn api_key_resolution_uses_keychain_without_environment_candidates() {
    let env_names = Vec::new();
    let resolved = resolve_api_key_from_env_names_with(
        &env_names,
        "cortexfs:api.foo-bar.com",
        "default",
        |_name| Err(std::env::VarError::NotPresent),
        |service, account| {
            assert_eq!(service, "cortexfs:api.foo-bar.com");
            assert_eq!(account, "default");
            Ok(Some("keychain-secret".to_owned()))
        },
    );
    assert_eq!(resolved, Ok(Some("keychain-secret".to_owned())));
}

#[test]
fn api_key_resolution_reports_unconfigured_without_environment_or_keychain() {
    let resolved = resolve_api_key_with(
        "CTX_LMM_SECRET",
        "cortexfs:lmm",
        "default",
        |_name| Err(std::env::VarError::NotPresent),
        |_service, _account| Ok(None),
    );
    assert_eq!(resolved, Ok(None));

    let invalid = resolve_api_key_with(
        "BAD-NAME",
        "cortexfs:lmm",
        "default",
        |_name| Err(std::env::VarError::NotPresent),
        |_service, _account| Ok(None),
    );
    assert_eq!(invalid, Err(ApiKeyResolutionError::InvalidName));

    let invalid_service = resolve_api_key_with(
        "CTX_LMM_SECRET",
        "cortexfs:\u{1b}lmm",
        "default",
        |_name| Err(std::env::VarError::NotPresent),
        |_service, _account| Ok(None),
    );
    assert_eq!(invalid_service, Err(ApiKeyResolutionError::InvalidName));
}
