use super::*;
use std::cell::Cell;

#[test]
fn raw_runtime_slots_cannot_read_auth_profile_bundles() {
    let reads = Cell::new(0);
    for slot in ["auth-work", "auth-default"] {
        assert!(!runtime_secret_env_matches("fixture", slot, &|_| {
            reads.set(reads.get() + 1);
            Err(env::VarError::NotPresent)
        }));
    }
    assert_eq!(reads.get(), 0);
}

#[test]
fn anonymous_local_store_denial_never_hides_refresh_or_profile_errors() {
    use cortexfs::AuthProviderError::{InvalidCredential, StoreAccessDenied, Unavailable};
    for transport in [
        ResolvedTransport::Direct {
            base_url: "http://localhost/v1".to_owned(),
        },
        ResolvedTransport::Unix {
            base_url: "http://localhost/v1".to_owned(),
            socket_path: "/fixture.sock".to_owned(),
        },
    ] {
        assert!(anonymous_store_error(
            StoreAccessDenied,
            None,
            &transport,
            1000
        ));
        for (error, slot, uid) in [
            (Unavailable, None, 1000),
            (InvalidCredential, None, 1000),
            (StoreAccessDenied, Some("work"), 1000),
            (StoreAccessDenied, None, 0),
        ] {
            assert!(!anonymous_store_error(error, slot, &transport, uid));
        }
    }
    let remote = ResolvedTransport::Direct {
        base_url: "https://api.example.com/v1".to_owned(),
    };
    assert!(!anonymous_store_error(
        StoreAccessDenied,
        None,
        &remote,
        1000
    ));
}

#[test]
fn profile_api_key_uses_anthropic_header_shape() {
    let credential = cortexfs::Credential::ApiKey {
        provider: "anthropic".to_owned(),
        key: "secret".to_owned(),
        slot: None,
    };
    assert_eq!(
        profile_credential(&credential, ProviderRuntimeDriver::Anthropic, false),
        Ok(Some(ProviderCredential::AnthropicApiKey(
            "secret".to_owned()
        )))
    );
}

#[test]
fn profile_codex_rejects_non_responses_driver() {
    let credential = cortexfs::Credential::OAuth {
        provider: "codex".to_owned(),
        access_token: "secret".to_owned(),
        refresh_token: None,
        expires_at: None,
        scopes: Vec::new(),
    };
    assert_eq!(
        profile_credential(&credential, ProviderRuntimeDriver::OpenAiChat, true),
        Err("Codex OAuth only supports openai.responses".to_owned())
    );
}

#[test]
fn egress_codex_request_keeps_subscription_url_without_host_secrets() -> Result<(), String> {
    let transport = ResolvedTransport::Unix {
        base_url: "http://localhost/backend-api/codex".to_owned(),
        socket_path: "/relay/codex.sock".to_owned(),
    };
    let credential = ProviderCredential::Egress {
        token: "local-capability".to_owned(),
        codex: true,
    };
    let (target, headers) = openai_request_target(&transport, Some(&credential), true, "run")?;
    assert_eq!(target.url, "http://localhost/backend-api/codex/responses");
    assert_eq!(headers, ["Authorization: Bearer local-capability"]);
    assert!(openai_request_target(&transport, Some(&credential), false, "run").is_err());
    Ok(())
}
