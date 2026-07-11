#[test]
fn oauth_pkce_uses_rfc7636_s256_vector() {
    let pkce = OAuthPkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk");
    let pkce = ok!(pkce);

    assert_eq!(OAuthPkce::method(), "S256");
    assert_eq!(
        pkce.challenge(),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn oauth_authorization_url_and_token_form_include_pkce() {
    let config = test_oauth_config();
    let pkce = ok!(OAuthPkce::from_verifier(
        "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
    ));

    let url = ok!(oauth_authorization_url(&config, "state value", &pkce));
    assert!(url.starts_with("https://auth.example/authorize?"));
    assert!(url.contains("response_type=code"));
    assert!(url.contains("client_id=client-1"));
    assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A8765%2Fcallback"));
    assert!(url.contains("scope=model.read%20offline_access"));
    assert!(url.contains("state=state%20value"));
    assert!(url.contains("code_challenge_method=S256"));
    assert!(url.contains("code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"));

    let form = ok!(oauth_authorization_code_form(&config, "auth code", &pkce));
    assert!(form.contains("grant_type=authorization_code"));
    assert!(form.contains("code=auth%20code"));
    assert!(form.contains("code_verifier=dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"));
}

#[test]
fn oauth_forms_reject_control_characters() {
    let mut config = test_oauth_config();
    let pkce = ok!(OAuthPkce::from_verifier(
        "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"
    ));

    assert_eq!(
        oauth_authorization_url(&config, "state\u{1b}", &pkce),
        Err(OAuthError::InvalidConfig)
    );
    assert_eq!(
        oauth_authorization_code_form(&config, "code\r", &pkce),
        Err(OAuthError::InvalidConfig)
    );
    assert_eq!(
        oauth_refresh_token_form(&config, "refresh\u{1b}"),
        Err(OAuthError::InvalidConfig)
    );

    config.scopes.push("bad\u{1b}scope".to_owned());
    assert_eq!(
        oauth_authorization_url(&config, "state", &pkce),
        Err(OAuthError::InvalidConfig)
    );
}

#[test]
fn oauth_token_response_accepts_bearer_and_rejects_other_types() {
    let token = ok!(parse_oauth_token_response(
        br#"{"access_token":"access-1","token_type":"Bearer","expires_in":3600,"refresh_token":"refresh-1"}"#
    ));

    assert_eq!(token.access_token, "access-1");
    assert_eq!(token.refresh_token.as_deref(), Some("refresh-1"));
    assert!(
        parse_oauth_token_response(br#"{"access_token":"access-1","token_type":"mac"}"#).is_err()
    );
}

#[test]
fn oauth_access_token_resolution_prefers_environment_over_keychain() {
    let config = test_oauth_config();
    let resolved = resolve_oauth_access_token_with(
        "openai",
        &config,
        |name| {
            if name == "CTX_OPENAI_OAUTH_ACCESS_TOKEN" {
                Ok("env-access".to_owned())
            } else {
                Err(std::env::VarError::NotPresent)
            }
        },
        |_service, _account| Ok(Some("keychain-access".to_owned())),
    );

    assert_eq!(resolved, Ok(Some("env-access".to_owned())));

    let fallback = resolve_oauth_access_token_with(
        "openai",
        &config,
        |_name| Err(std::env::VarError::NotPresent),
        |service, account| {
            assert_eq!(service, "cortexfs:openai");
            assert_eq!(account, "oauth:access");
            Ok(Some("keychain-access".to_owned()))
        },
    );
    assert_eq!(fallback, Ok(Some("keychain-access".to_owned())));

    let invalid_provider = resolve_oauth_access_token_with(
        "openai\u{1b}",
        &config,
        |_name| Err(std::env::VarError::NotPresent),
        |_service, _account| Ok(Some("keychain-access".to_owned())),
    );
    assert_eq!(invalid_provider, Err(OAuthError::InvalidConfig));
}

fn test_oauth_config() -> OAuthProviderConfig {
    OAuthProviderConfig {
        client_id: "client-1".to_owned(),
        auth_url: "https://auth.example/authorize".to_owned(),
        token_url: "https://auth.example/token".to_owned(),
        redirect_uri: "http://127.0.0.1:8765/callback".to_owned(),
        scopes: vec!["model.read".to_owned(), "offline_access".to_owned()],
        access_token_account: None,
        refresh_token_account: None,
    }
}
use super::*;
