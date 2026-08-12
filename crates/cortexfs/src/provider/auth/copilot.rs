use super::adapter::{
    AuthProvider, AuthProviderError, AuthRequest, AuthTransport, DeviceChallenge, default_login,
    default_models, default_refresh, device_request,
};
use super::common::AdapterCore;
use super::device::DeviceConfig;
use super::{Credential, ProviderAuthConfig};
use crate::provider::oauth::{OAuthPkce, OAuthProviderConfig};

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
impl AuthProvider for GitHubCopilotAdapter {
    fn id(&self) -> &str {
        &self.core.id
    }

    fn methods(&self) -> &[ProviderAuthConfig] {
        &self.core.methods
    }

    fn aliases(&self) -> &[String] {
        &self.core.aliases
    }

    fn authorization_url(
        &self,
        state: &str,
        pkce: &OAuthPkce,
    ) -> Result<String, AuthProviderError> {
        self.core.authorization(state, pkce)
    }

    fn login(&self, request: AuthRequest) -> Result<Credential, AuthProviderError> {
        default_login(self, request)
    }

    fn login_with(
        &self,
        request: AuthRequest,
        transport: &mut dyn AuthTransport,
        now: u64,
    ) -> Result<Credential, AuthProviderError> {
        if let Some(credential) = device_request(self, &request, transport, now)? {
            return Ok(credential);
        }
        self.core.login(request, transport, now)
    }

    fn persist(&self, credential: &Credential, now: u64) -> Result<(), AuthProviderError> {
        self.core.persist(credential, now)
    }

    fn device_login_with(
        &self,
        timeout_secs: u64,
        transport: &mut dyn AuthTransport,
        now: u64,
        notify: &mut dyn FnMut(&DeviceChallenge),
        pause: &mut dyn FnMut(u64),
    ) -> Result<Credential, AuthProviderError> {
        self.core
            .device_login(timeout_secs, transport, now, notify, pause)
    }

    fn refresh(&self, credential: &Credential) -> Result<Credential, AuthProviderError> {
        default_refresh(self, credential)
    }

    fn refresh_with(
        &self,
        credential: &Credential,
        transport: &mut dyn AuthTransport,
        now: u64,
    ) -> Result<Credential, AuthProviderError> {
        self.core.refresh(credential, transport, now)
    }

    fn models(&self, credential: Option<&Credential>) -> Result<Vec<String>, AuthProviderError> {
        default_models(self, credential)
    }

    fn models_with(
        &self,
        credential: Option<&Credential>,
        transport: &mut dyn AuthTransport,
    ) -> Result<Vec<String>, AuthProviderError> {
        self.core.models(credential, transport, "Authorization")
    }

    fn model_headers(
        &self,
        credential: &Credential,
    ) -> Result<Vec<(String, String)>, AuthProviderError> {
        self.core.model_headers(credential, "Authorization")
    }
}
