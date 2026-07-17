use super::*;
use crate::object::runner::{
    ProviderCredential, ResolvedTransport, responses::openai_request_target,
};
use crate::{
    OAuthTokenState, codex_oauth_config, exchange_oauth_token_with, oauth_needs_refresh,
    oauth_token_state, resolve_codex_with, store_codex_with,
};

fn config() -> OAuthProviderConfig {
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

#[test]
fn oauth_pkce_authorization_and_forms_are_bounded() {
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    let pkce = ok!(OAuthPkce::from_verifier(verifier));
    assert_eq!(
        pkce.challenge(),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let url = ok!(oauth_authorization_url(&config(), "state value", &pkce));
    for part in [
        "client_id=client-1",
        "state=state%20value",
        "code_challenge_method=S256",
    ] {
        assert!(url.contains(part));
    }
    let form = ok!(oauth_authorization_code_form(&config(), "auth code", &pkce));
    assert!(form.contains("code=auth%20code"));
    assert_eq!(
        oauth_refresh_token_form(&config(), "bad\r"),
        Err(OAuthError::InvalidConfig)
    );
}

#[test]
fn oauth_token_exchange_is_hermetic_and_validates_bearer() {
    let token = ok!(exchange_oauth_token_with(&config(), "form", |url, body| {
        assert_eq!((url, body), ("https://auth.example/token", "form"));
        Ok((
            200,
            br#"{"access_token":"access","token_type":"Bearer"}"#.to_vec(),
        ))
    }));
    assert_eq!(token.access_token, "access");
    assert!(parse_oauth_token_response(br#"{"access_token":"x","token_type":"mac"}"#).is_err());
}

#[test]
fn oauth_access_resolution_prefers_environment() {
    let resolved = resolve_oauth_access_token_with(
        "openai",
        &config(),
        |name| {
            (name == "CTX_OPENAI_OAUTH_ACCESS_TOKEN")
                .then(|| "env".to_owned())
                .ok_or(std::env::VarError::NotPresent)
        },
        |_service, _account| Ok(Some("keyring".to_owned())),
    );
    assert_eq!(resolved, Ok(Some("env".to_owned())));
    assert_eq!(
        resolve_oauth_access_token_with(
            "bad\u{1b}",
            &config(),
            |_name| Err(std::env::VarError::NotPresent),
            |_service, _account| Ok(None)
        ),
        Err(OAuthError::InvalidConfig)
    );
}

#[test]
fn codex_jwt_expiry_and_root_storage_are_complete() {
    let jwt = "e30.eyJjaGF0Z3B0X2FjY291bnRfaWQiOiJhY2N0LTEiLCJleHAiOjEzMDB9.x";
    let token = ok!(parse_oauth_token_response(
        format!(r#"{{"access_token":"{jwt}","expires_in":300,"refresh_token":"refresh"}}"#)
            .as_bytes()
    ));
    let state = ok!(oauth_token_state(&token, None, 1_000));
    assert_eq!(
        (&state.account_id, state.expires_at),
        (&"acct-1".to_owned(), 1_300)
    );
    assert!(oauth_needs_refresh(state.expires_at, 1_000));
    let mut stored = Vec::new();
    ok!(store_codex_with(&state, |slot, value| {
        stored.push((slot.to_owned(), value.to_owned()));
        Ok(())
    }));
    assert_eq!(
        stored
            .iter()
            .map(|value| value.0.as_str())
            .collect::<Vec<_>>(),
        ["default", "oauth-refresh", "oauth-account", "oauth-expires"]
    );
}

#[test]
fn codex_refresh_retains_complete_state_and_fails_closed() {
    let stored = OAuthTokenState {
        access_token: "old".to_owned(),
        refresh_token: "refresh".to_owned(),
        account_id: "account".to_owned(),
        expires_at: 1_100,
    };
    let mut refreshed = Some(stored.clone());
    let credential = ok!(resolve_codex_with(
        &codex_oauth_config(),
        &mut refreshed,
        1_000,
        |form| {
            assert!(form.contains("refresh_token=refresh"));
            parse_oauth_token_response(br#"{"access_token":"new","expires_in":600}"#)
        }
    ));
    assert_eq!(credential.map(|value| value.0), Some("new".to_owned()));
    assert_eq!(
        refreshed.map(|value| value.refresh_token),
        Some("refresh".to_owned())
    );
    let mut invalid = Some(stored);
    if let Some(state) = invalid.as_mut() {
        state.refresh_token.clear();
    }
    assert_eq!(
        resolve_codex_with(&codex_oauth_config(), &mut invalid, 1_000, |_form| Err(
            OAuthError::Transport
        )),
        Err(OAuthError::InvalidToken)
    );
}

#[test]
fn codex_request_uses_fixed_direct_backend_and_unix_transport() {
    let credential = ProviderCredential::Codex {
        token: "access".into(),
        account_id: "account".into(),
    };
    let direct = ResolvedTransport::Direct {
        base_url: "https://ignored/v1".into(),
    };
    let (target, headers) = ok!(openai_request_target(
        &direct,
        Some(&credential),
        true,
        "run-1"
    ));
    assert_eq!(
        target.url,
        "https://chatgpt.com/backend-api/codex/responses"
    );
    assert!(
        [
            "Authorization: Bearer access",
            "ChatGPT-Account-Id: account",
            "originator: ctx",
            "session-id: run-1"
        ]
        .iter()
        .all(|expected| headers.iter().any(|value| value == expected))
    );
    let unix = ResolvedTransport::Unix {
        base_url: "http://localhost/backend-api/codex".into(),
        socket_path: "/run/codex.sock".into(),
    };
    assert_eq!(
        ok!(openai_request_target(
            &unix,
            Some(&credential),
            true,
            "run-1"
        ))
        .0
        .url,
        "http://localhost/backend-api/codex/responses"
    );
    assert!(openai_request_target(&direct, Some(&credential), false, "run-1").is_err());
}
