use super::*;

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
