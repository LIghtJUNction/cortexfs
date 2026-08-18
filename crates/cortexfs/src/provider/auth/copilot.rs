use super::ProviderAuthConfig;
use super::common::{AdapterCore, CoreAuthProvider};
use super::device::DeviceConfig;
use crate::provider::oauth::OAuthProviderConfig;
/// GitHub OAuth adapter for Copilot-compatible model endpoints.
#[derive(Debug)]
pub struct GitHubCopilotAdapter {
    core: AdapterCore,
}
impl GitHubCopilotAdapter {
    /// Builds the GitHub OAuth metadata used by a Copilot client registration.
    #[must_use]
    pub fn oauth_config(
        client_id: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> OAuthProviderConfig {
        OAuthProviderConfig {
            client_id: client_id.into(),
            auth_url: "https://github.com/login/oauth/authorize".to_owned(),
            token_url: "https://github.com/login/oauth/access_token".to_owned(),
            redirect_uri: redirect_uri.into(),
            scopes: vec!["read:user".to_owned()],
            device: None,
            access_token_account: None,
            refresh_token_account: None,
        }
    }
    /// Builds a configured Copilot adapter; client registration stays host-owned.
    #[must_use]
    pub fn new(oauth: OAuthProviderConfig) -> Self {
        Self::with_base(oauth, "https://api.githubcopilot.com")
    }
    /// Builds a Copilot adapter using the configured provider base URL.
    #[must_use]
    pub fn with_base(oauth: OAuthProviderConfig, base_url: &str) -> Self {
        Self::with_config("github-copilot", base_url, default_methods(), oauth)
    }

    /// Builds a Copilot adapter from host identity and authentication metadata.
    #[must_use]
    pub fn with_config(
        id: impl Into<String>,
        base_url: &str,
        methods: Vec<ProviderAuthConfig>,
        oauth: OAuthProviderConfig,
    ) -> Self {
        let id = id.into();
        let aliases = match id.as_str() {
            "github-copilot" => vec!["copilot".to_owned()],
            "copilot" => vec!["github-copilot".to_owned()],
            _ => Vec::new(),
        };
        let device = oauth.device.clone().map_or_else(
            || DeviceConfig {
                request_url: "https://github.com/login/device/code".to_owned(),
                token_url: "https://github.com/login/oauth/access_token".to_owned(),
                verification_uri: "https://github.com/login/device".to_owned(),
            },
            DeviceConfig::from,
        );
        Self {
            core: AdapterCore {
                id,
                aliases,
                model_url: format!("{}/models", base_url.trim_end_matches('/')),
                methods: if methods.is_empty() {
                    default_methods()
                } else {
                    methods
                },
                oauth: Some(oauth),
                device: Some(device),
            },
        }
    }
}

fn default_methods() -> Vec<ProviderAuthConfig> {
    vec![
        ProviderAuthConfig::oauth(super::OAuthFlow::AuthorizationCode, "subscription"),
        ProviderAuthConfig::oauth(super::OAuthFlow::DeviceCode, "subscription"),
    ]
}
impl CoreAuthProvider for GitHubCopilotAdapter {
    fn core(&self) -> &AdapterCore {
        &self.core
    }
}
