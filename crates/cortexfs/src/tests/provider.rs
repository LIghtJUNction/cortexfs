use crate::{
    AnthropicAdapter, AuthMethod, AuthProvider, AuthProviderError, AuthRequest, AuthResponse,
    AuthTransport, Credential, CredentialKind, GitHubCopilotAdapter, OAuthDeviceConfig, OAuthFlow,
    OAuthPkce, OAuthProviderConfig, OpenAiAdapter, ProviderAuthConfig, ProviderRegistry,
    configured_adapter, effective_auth_methods,
};

#[derive(Default)]
struct ScriptedTransport {
    responses: Vec<AuthResponse>,
    posts: Vec<(String, String, String)>,
    gets: Vec<(String, Headers)>,
}

type Headers = Vec<(String, String)>;

impl AuthTransport for ScriptedTransport {
    fn post(
        &mut self,
        url: &str,
        content_type: &str,
        body: &str,
    ) -> Result<AuthResponse, AuthProviderError> {
        self.posts
            .push((url.to_owned(), content_type.to_owned(), body.to_owned()));
        self.responses.pop().ok_or(AuthProviderError::Unavailable)
    }

    fn get(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<AuthResponse, AuthProviderError> {
        self.gets.push((
            url.to_owned(),
            headers
                .iter()
                .map(|&(name, value)| (name.to_owned(), value.to_owned()))
                .collect(),
        ));
        self.responses.pop().ok_or(AuthProviderError::Unavailable)
    }
}

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
    let key = Credential::ApiKey {
        provider: "anthropic".to_owned(),
        key: "sk-test".to_owned(),
        slot: Some("secondary".to_owned()),
    };
    let restored_key: Credential = serde_json::from_str(&serde_json::to_string(&key)?)?;
    assert_eq!(restored_key, key);
    Ok(())
}

#[test]
fn auth_config_rejects_unsafe_slot_names() {
    let config = ProviderAuthConfig::api_key("../secret");
    assert!(!config.is_valid());
}

#[test]
fn openai_adapter_exchanges_refreshes_and_discovers_models() -> Result<(), AuthProviderError> {
    let adapter = OpenAiAdapter::codex();
    let verifier = "a".repeat(43);
    let mut transport = ScriptedTransport {
        responses: vec![
            AuthResponse {
                status: 200,
                body: br#"{"data":[{"id":"gpt-5-codex"},{"id":"gpt-5-codex"}]}"#.to_vec(),
            },
            AuthResponse {
                status: 200,
                body: br#"{"access_token":"new","expires_in":300}"#.to_vec(),
            },
            AuthResponse {
                status: 200,
                body: br#"{"access_token":"access","refresh_token":"refresh","expires_in":600,"scope":"model.read"}"#.to_vec(),
            },
        ],
        ..ScriptedTransport::default()
    };
    let credential = adapter.login_with(
        AuthRequest::AuthorizationCodePkce {
            code: "code".to_owned(),
            verifier,
        },
        &mut transport,
        100,
    )?;
    let refreshed = adapter.refresh_with(&credential, &mut transport, 200)?;
    assert_eq!(refreshed.provider(), "codex");
    assert!(matches!(
        &refreshed,
        Credential::OAuth { access_token, .. } if access_token == "new"
    ));
    let models = adapter.models_with(Some(&refreshed), &mut transport)?;
    assert_eq!(models, ["gpt-5-codex"]);
    assert_eq!(transport.posts.len(), 2);
    assert!(
        transport
            .posts
            .first()
            .is_some_and(|post| post.2.contains("grant_type=authorization_code"))
    );
    Ok(())
}

#[test]
fn codex_device_flow_keeps_legacy_challenge_contract() -> Result<(), AuthProviderError> {
    let adapter = OpenAiAdapter::codex();
    let verifier = "a".repeat(43);
    let challenge = serde_json::json!({
        "device_auth_id": "device",
        "user_code": "ABCD-1234",
        "interval": "1"
    });
    let grant = serde_json::json!({
        "authorization_code": "code",
        "code_verifier": verifier
    });
    let mut transport = ScriptedTransport {
        responses: vec![
            AuthResponse {
                status: 200,
                body: br#"{"access_token":"access","token_type":"bearer"}"#.to_vec(),
            },
            AuthResponse {
                status: 200,
                body: grant.to_string().into_bytes(),
            },
            AuthResponse {
                status: 200,
                body: challenge.to_string().into_bytes(),
            },
        ],
        ..ScriptedTransport::default()
    };
    let mut challenges = Vec::new();
    let mut pauses = Vec::new();
    let credential = adapter.device_login_with(
        30,
        &mut transport,
        100,
        &mut |value| challenges.push(value.clone()),
        &mut |seconds| pauses.push(seconds),
    )?;
    assert!(
        matches!(credential, Credential::OAuth { ref access_token, .. } if access_token == "access")
    );
    assert_eq!(
        challenges.first().map(|value| value.user_code.as_str()),
        Some("ABCD-1234")
    );
    assert!(pauses.is_empty());
    assert_eq!(transport.posts.len(), 3);
    Ok(())
}

#[test]
fn anthropic_adapter_uses_api_key_header() -> Result<(), AuthProviderError> {
    let adapter = AnthropicAdapter::claude();
    let mut transport = ScriptedTransport {
        responses: vec![AuthResponse {
            status: 200,
            body: br#"{"data":[{"id":"claude-sonnet"}]}"#.to_vec(),
        }],
        ..ScriptedTransport::default()
    };
    let credential = adapter.login_with(
        AuthRequest::ApiKey {
            slot: "default".to_owned(),
            key: "sk-test".to_owned(),
        },
        &mut transport,
        0,
    )?;
    assert_eq!(
        adapter.models_with(Some(&credential), &mut transport)?,
        ["claude-sonnet"]
    );
    assert_eq!(
        transport.gets.first().map(|get| &get.1),
        Some(&vec![
            ("x-api-key".to_owned(), "sk-test".to_owned()),
            ("anthropic-version".to_owned(), "2023-06-01".to_owned()),
        ])
    );
    Ok(())
}

#[test]
fn anthropic_oauth_adapter_exchanges_refreshes_and_discovers_models()
-> Result<(), AuthProviderError> {
    let oauth = OAuthProviderConfig {
        client_id: "claude-client".to_owned(),
        auth_url: "https://auth.example/authorize".to_owned(),
        token_url: "https://auth.example/token".to_owned(),
        redirect_uri: "http://127.0.0.1:8765/callback".to_owned(),
        scopes: vec!["model.read".to_owned()],
        device: None,
        access_token_account: None,
        refresh_token_account: None,
    };
    let adapter = AnthropicAdapter::new(
        "anthropic",
        "https://api.example/v1",
        vec![ProviderAuthConfig::oauth(
            OAuthFlow::AuthorizationCode,
            "subscription",
        )],
        Some(oauth),
    );
    let mut transport = ScriptedTransport {
        responses: vec![
            AuthResponse {
                status: 200,
                body: br#"{"data":[{"id":"claude-sonnet"}]}"#.to_vec(),
            },
            AuthResponse {
                status: 200,
                body: br#"{"access_token":"new","expires_in":300}"#.to_vec(),
            },
            AuthResponse {
                status: 200,
                body: br#"{"access_token":"access","refresh_token":"refresh","expires_in":600}"#
                    .to_vec(),
            },
        ],
        ..ScriptedTransport::default()
    };
    let credential = adapter.login_with(
        AuthRequest::AuthorizationCodePkce {
            code: "code".to_owned(),
            verifier: "a".repeat(43),
        },
        &mut transport,
        100,
    )?;
    let refreshed = adapter.refresh_with(&credential, &mut transport, 200)?;
    assert!(
        matches!(refreshed, Credential::OAuth { ref access_token, .. } if access_token == "new")
    );
    assert_eq!(
        adapter.models_with(Some(&refreshed), &mut transport)?,
        ["claude-sonnet"]
    );
    assert_eq!(
        transport.gets.first().map(|get| &get.1),
        Some(&vec![
            ("Authorization".to_owned(), "Bearer new".to_owned()),
            ("anthropic-version".to_owned(), "2023-06-01".to_owned()),
        ])
    );
    Ok(())
}

#[test]
fn anthropic_host_device_flow_uses_shared_adapter_contract() -> Result<(), AuthProviderError> {
    let oauth = OAuthProviderConfig {
        client_id: "claude-client".to_owned(),
        auth_url: "https://auth.example/authorize".to_owned(),
        token_url: "https://auth.example/token".to_owned(),
        redirect_uri: "http://127.0.0.1:8765/callback".to_owned(),
        scopes: vec!["model.read".to_owned()],
        device: Some(OAuthDeviceConfig {
            request_url: "https://auth.example/device".to_owned(),
            token_url: "https://auth.example/device/token".to_owned(),
            verification_uri: "https://auth.example/verify".to_owned(),
        }),
        access_token_account: None,
        refresh_token_account: None,
    };
    let adapter = AnthropicAdapter::new(
        "claude",
        "https://api.example/v1",
        vec![ProviderAuthConfig::oauth(
            OAuthFlow::DeviceCode,
            "subscription",
        )],
        Some(oauth),
    );
    let mut transport = ScriptedTransport {
        responses: vec![
            AuthResponse {
                status: 200,
                body: br#"{"access_token":"access","token_type":"bearer"}"#.to_vec(),
            },
            AuthResponse {
                status: 200,
                body:
                    br#"{"device_code":"device","user_code":"ABCD","expires_in":30,"interval":1}"#
                        .to_vec(),
            },
        ],
        ..ScriptedTransport::default()
    };
    let credential = adapter.login_with(
        AuthRequest::DeviceCode { timeout_secs: 10 },
        &mut transport,
        100,
    )?;
    assert_eq!(credential.provider(), "claude");
    Ok(())
}

#[test]
fn copilot_device_flow_reports_challenge_and_refreshes_poll_interval()
-> Result<(), AuthProviderError> {
    let adapter = GitHubCopilotAdapter::new(GitHubCopilotAdapter::oauth_config(
        "client",
        "http://localhost/callback",
    ));
    let mut transport = ScriptedTransport {
        responses: vec![
            AuthResponse {
                status: 200,
                body: br#"{"access_token":"access","token_type":"bearer","expires_in":60}"#
                    .to_vec(),
            },
            AuthResponse {
                status: 400,
                body: br#"{"error":"authorization_pending"}"#.to_vec(),
            },
            AuthResponse {
                status: 200,
                body: br#"{"device_code":"device","user_code":"ABCD-1234","verification_uri":"https://github.com/login/device","expires_in":30,"interval":1}"#.to_vec(),
            },
        ],
        ..ScriptedTransport::default()
    };
    let mut challenges = Vec::new();
    let mut pauses = Vec::new();
    let credential = adapter.device_login_with(
        10,
        &mut transport,
        100,
        &mut |challenge| challenges.push(challenge.clone()),
        &mut |seconds| pauses.push(seconds),
    )?;
    assert!(
        matches!(credential, Credential::OAuth { ref access_token, .. } if access_token == "access")
    );
    assert_eq!(
        challenges
            .first()
            .map(|challenge| challenge.user_code.as_str()),
        Some("ABCD-1234")
    );
    assert_eq!(pauses, [1]);
    assert_eq!(transport.posts.len(), 3);
    Ok(())
}

#[test]
fn copilot_factory_preserves_host_identity_methods_and_oauth_runtime()
-> Result<(), AuthProviderError> {
    let oauth = GitHubCopilotAdapter::oauth_config("client", "http://localhost/callback");
    let methods = vec![ProviderAuthConfig::oauth(
        OAuthFlow::AuthorizationCode,
        "subscription",
    )];
    let adapter = configured_adapter(
        "copilot",
        "https://api.example/v1",
        methods.clone(),
        Some(oauth),
    )
    .ok_or(AuthProviderError::Unavailable)?;
    assert_eq!(adapter.id(), "copilot");
    assert_eq!(adapter.aliases(), &["github-copilot".to_owned()]);
    assert_eq!(adapter.methods(), methods.as_slice());
    let mut transport = ScriptedTransport {
        responses: vec![
            AuthResponse {
                status: 200,
                body: br#"{"data":[{"id":"copilot-chat"}]}"#.to_vec(),
            },
            AuthResponse {
                status: 200,
                body: br#"{"access_token":"new","expires_in":300}"#.to_vec(),
            },
            AuthResponse {
                status: 200,
                body: br#"{"access_token":"access","refresh_token":"refresh","expires_in":600}"#
                    .to_vec(),
            },
        ],
        ..ScriptedTransport::default()
    };
    let credential = adapter.login_with(
        AuthRequest::AuthorizationCodePkce {
            code: "code".to_owned(),
            verifier: "a".repeat(43),
        },
        &mut transport,
        100,
    )?;
    assert_eq!(credential.provider(), "copilot");
    let refreshed = adapter.refresh_with(&credential, &mut transport, 200)?;
    assert!(
        matches!(refreshed, Credential::OAuth { ref access_token, .. } if access_token == "new")
    );
    assert_eq!(
        adapter.models_with(Some(&refreshed), &mut transport)?,
        ["copilot-chat"]
    );
    Ok(())
}

#[test]
fn registry_resolves_concrete_provider_aliases() -> Result<(), Box<dyn std::error::Error>> {
    let config = GitHubCopilotAdapter::oauth_config("client", "http://localhost/callback");
    let registry = ProviderRegistry::with_defaults(Some(config))?;
    assert_eq!(registry.names(), ["codex", "anthropic", "github-copilot"]);
    assert_eq!(
        registry.get("claude").map(AuthProvider::id),
        Some("anthropic")
    );
    assert_eq!(
        registry.get("copilot").map(AuthProvider::id),
        Some("github-copilot")
    );
    Ok(())
}

#[test]
fn host_device_endpoints_feed_generic_adapter() -> Result<(), AuthProviderError> {
    let oauth = OAuthProviderConfig {
        client_id: "client".to_owned(),
        auth_url: "https://auth.example/authorize".to_owned(),
        token_url: "https://auth.example/token".to_owned(),
        redirect_uri: "http://127.0.0.1:8765/callback".to_owned(),
        scopes: vec!["model.read".to_owned()],
        device: Some(OAuthDeviceConfig {
            request_url: "https://auth.example/device".to_owned(),
            token_url: "https://auth.example/device/token".to_owned(),
            verification_uri: "https://auth.example/verify".to_owned(),
        }),
        access_token_account: None,
        refresh_token_account: None,
    };
    let adapter = OpenAiAdapter::new(
        "example",
        "https://api.example/v1",
        vec![ProviderAuthConfig::oauth(
            OAuthFlow::DeviceCode,
            "subscription",
        )],
        Some(oauth),
    );
    let mut transport = ScriptedTransport {
        responses: vec![
            AuthResponse {
                status: 200,
                body: br#"{"access_token":"access","token_type":"bearer"}"#.to_vec(),
            },
            AuthResponse {
                status: 200,
                body:
                    br#"{"device_code":"device","user_code":"ABCD","expires_in":30,"interval":1}"#
                        .to_vec(),
            },
        ],
        ..ScriptedTransport::default()
    };
    let credential = adapter.login_with(
        AuthRequest::DeviceCode { timeout_secs: 10 },
        &mut transport,
        100,
    )?;
    assert_eq!(credential.provider(), "example");
    assert_eq!(
        transport.posts.first().map(|post| post.0.as_str()),
        Some("https://auth.example/device")
    );
    assert_eq!(
        transport.posts.get(1).map(|post| post.0.as_str()),
        Some("https://auth.example/device/token")
    );
    Ok(())
}

#[test]
fn authorization_url_requires_authorization_code_method() -> Result<(), AuthProviderError> {
    let oauth = OAuthProviderConfig {
        client_id: "client".to_owned(),
        auth_url: "https://auth.example/authorize".to_owned(),
        token_url: "https://auth.example/token".to_owned(),
        redirect_uri: "http://127.0.0.1:8765/callback".to_owned(),
        scopes: Vec::new(),
        device: None,
        access_token_account: None,
        refresh_token_account: None,
    };
    let adapter = OpenAiAdapter::new(
        "example",
        "https://api.example/v1",
        vec![ProviderAuthConfig::oauth(
            OAuthFlow::DeviceCode,
            "subscription",
        )],
        Some(oauth),
    );
    let pkce = OAuthPkce::from_verifier(&"a".repeat(43))
        .map_err(|_error| AuthProviderError::InvalidConfig)?;
    assert_eq!(
        adapter.authorization_url("state", &pkce),
        Err(AuthProviderError::UnsupportedMethod)
    );
    Ok(())
}

#[test]
fn model_headers_reject_control_character_credentials() {
    let adapter = AnthropicAdapter::claude();
    let credential = Credential::ApiKey {
        provider: "anthropic".to_owned(),
        key: "bad\nkey".to_owned(),
        slot: Some("default".to_owned()),
    };
    assert_eq!(
        adapter.model_headers(&credential),
        Err(AuthProviderError::InvalidCredential)
    );
}

#[test]
fn api_key_only_adapter_does_not_refresh_oauth_credentials() {
    let adapter = AnthropicAdapter::claude();
    let credential = Credential::OAuth {
        provider: "anthropic".to_owned(),
        access_token: "access".to_owned(),
        refresh_token: Some("refresh".to_owned()),
        expires_at: None,
        scopes: Vec::new(),
    };
    assert_eq!(
        adapter.refresh(&credential),
        Err(AuthProviderError::UnsupportedMethod)
    );
}
