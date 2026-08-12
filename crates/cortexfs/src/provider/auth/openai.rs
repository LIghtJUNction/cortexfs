use super::adapter::{
    AuthProvider, AuthProviderError, AuthRequest, AuthTransport, default_login, default_models,
    default_refresh, device_request,
};
use super::codexdevice;
use super::common::AdapterCore;
use super::device::{DeviceChallenge, DeviceConfig};
use super::model::model_url;
use super::{Credential, ProviderAuthConfig};
use crate::provider::oauth::{OAuthPkce, OAuthProviderConfig, codex_oauth_config};
/// OpenAI-compatible adapter, including the Codex subscription OAuth profile.
#[derive(Debug)]
pub struct OpenAiAdapter {
    core: AdapterCore,
}

impl OpenAiAdapter {
    /// Builds the built-in Codex OAuth adapter.
    #[must_use]
    pub fn codex() -> Self {
        let oauth = codex_oauth_config();
        Self::new(
            "codex",
            "https://chatgpt.com/backend-api/codex",
            vec![
                ProviderAuthConfig::oauth(super::OAuthFlow::AuthorizationCode, "subscription"),
                ProviderAuthConfig::oauth(super::OAuthFlow::DeviceCode, "subscription"),
            ],
            Some(oauth),
        )
    }
    /// Builds an `OpenAI` adapter from host-side provider metadata.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        base_url: &str,
        methods: Vec<ProviderAuthConfig>,
        oauth: Option<OAuthProviderConfig>,
    ) -> Self {
        let id = id.into();
        let aliases = match id.as_str() {
            "codex" => vec!["openai".to_owned()],
            "openai" => vec!["codex".to_owned()],
            _ => Vec::new(),
        };
        let device = oauth
            .as_ref()
            .and_then(|config| config.device.clone())
            .map(DeviceConfig::from);
        Self {
            core: AdapterCore {
                aliases,
                model_url: model_url(base_url),
                id,
                methods,
                oauth,
                device,
            },
        }
    }
}
impl AuthProvider for OpenAiAdapter {
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
        if self.core.id == "codex" {
            return codexdevice::login(&self.core.id, timeout_secs, transport, now, notify, pause);
        }
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
