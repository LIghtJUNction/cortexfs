use super::bind_credentials;
use crate::provider::ProviderConfig;
use crate::provider::auth::{AuthProviderError, Credential};
use crate::runtime::egress::secret::ProviderEgressCredential;
use crate::runtime::egress::{ProviderEgressError, ProviderTarget};

#[test]
fn fallback_targets_resolve_separate_profiles_and_bind_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let configs = ["primary", "fallback"]
        .map(|name| {
            serde_json::from_value::<ProviderConfig>(
                serde_json::json!({"name": name, "base_url": format!("https://{name}.test/v1")}),
            )
        })
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let mut targets = configs
        .iter()
        .enumerate()
        .map(|(index, config)| ProviderTarget {
            profile: if index == 0 { "default" } else { "work" }.to_owned(),
            provider: config.name.clone().unwrap_or_default(),
            base_url: config.base_url.clone(),
            authority: config.base_url.trim_end_matches("/v1").to_owned(),
            base_path: "/v1".to_owned(),
            credential: None,
        })
        .collect::<Vec<_>>();
    let mut providers = Vec::new();
    bind_credentials(&mut targets, &configs, &[], "run", |target, _config| {
        let provider = &target.provider;
        providers.push(format!("{provider}/{}", target.profile));
        Ok(Some(Credential::ApiKey {
            provider: provider.to_owned(),
            key: format!("{provider}-{}-key", target.profile),
            slot: None,
        }))
    })?;
    assert_eq!(providers, ["primary/default", "fallback/work"]);
    for target in &targets {
        assert_eq!(
            target.credential.as_ref().map(|value| value.token.as_str()),
            Some(format!("{}-{}-key", target.provider, target.profile).as_str())
        );
    }
    let target = targets.first_mut().ok_or("missing target")?;
    "https://wrong.test/v1".clone_into(&mut target.base_url);
    target.credential = None;
    let mut reads = 0;
    assert_eq!(
        bind_credentials(&mut targets, &configs, &[], "run", |_, _| {
            reads += 1;
            Err(AuthProviderError::Unavailable)
        }),
        Err(ProviderEgressError::AuthorityConflict)
    );
    assert_eq!(reads, 0);
    Ok(())
}

#[test]
fn codex_profile_preserves_subscription_path_and_account() -> Result<(), Box<dyn std::error::Error>>
{
    use base64::Engine;
    let config: ProviderConfig = serde_json::from_value(serde_json::json!({
        "name": "codex", "base_url": "https://chatgpt.com/backend-api/codex",
        "oauth": crate::codex_oauth_config()
    }))?;
    let mut targets = [ProviderTarget {
        provider: "codex".to_owned(),
        profile: "default".to_owned(),
        base_url: "https://chatgpt.com/backend-api/codex/v1".to_owned(),
        authority: "https://chatgpt.com".to_owned(),
        base_path: "/backend-api/codex/v1".to_owned(),
        credential: None,
    }];
    let token = format!(
        "e30.{}.sig",
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(br#"{"chatgpt_account_id":"account"}"#)
    );
    bind_credentials(&mut targets, &[config], &[], "run", |_, _| {
        Ok(Some(Credential::OAuth {
            provider: "codex".to_owned(),
            access_token: token.clone(),
            refresh_token: None,
            expires_at: None,
            scopes: Vec::new(),
        }))
    })?;
    assert_eq!(targets[0].base_path, "/backend-api/codex");
    assert_eq!(targets[0].base_url, "https://chatgpt.com/backend-api/codex");
    assert_eq!(
        targets[0]
            .credential
            .as_ref()
            .and_then(|value| value.codex_account_id.as_deref()),
        Some("account")
    );
    for account in ["", "  ", "account\r\nx-injected: value", "account\u{0085}"] {
        let token = format!(
            "e30.{}.sig",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(serde_json::to_vec(
                &serde_json::json!({"chatgpt_account_id": account})
            )?)
        );
        let credential = Credential::OAuth {
            provider: "codex".to_owned(),
            access_token: token,
            refresh_token: None,
            expires_at: None,
            scopes: Vec::new(),
        };
        assert!(matches!(
            ProviderEgressCredential::from_profile(credential, "run", true),
            Err(ProviderEgressError::CredentialUnavailable)
        ));
    }
    Ok(())
}
