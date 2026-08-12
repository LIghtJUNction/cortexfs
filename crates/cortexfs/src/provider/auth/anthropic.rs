use super::adapter::{
    AuthProvider, AuthProviderError, AuthRequest, AuthTransport, default_login, default_models,
    default_refresh, device_request,
};
use super::common::AdapterCore;
use super::device::DeviceConfig;
use super::model::model_url;
use super::{Credential, ProviderAuthConfig};
use crate::provider::oauth::{OAuthPkce, OAuthProviderConfig};

/// Anthropic/Claude adapter with provider-specific API-key headers.
#[derive(Debug)]
pub struct AnthropicAdapter {
    core: AdapterCore,
}

impl AnthropicAdapter {
    /// Builds the Claude API-key profile.
    #[must_use]
    pub fn claude() -> Self {
        Self::new(
            "anthropic",
            "https://api.anthropic.com/v1",
            vec![ProviderAuthConfig::api_key("default")],
            None,
        )
    }

    /// Builds a Claude profile with host-supplied OAuth metadata.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        base_url: &str,
        methods: Vec<ProviderAuthConfig>,
        oauth: Option<OAuthProviderConfig>,
    ) -> Self {
        let id = id.into();
        let device = oauth
            .as_ref()
            .and_then(|config| config.device.clone())
            .map(DeviceConfig::from);
        Self {
            core: AdapterCore {
                aliases: vec!["claude".to_owned()],
                model_url: model_url(base_url),
                id,
                methods,
                oauth,
                device,
            },
        }
    }
}

impl AuthProvider for AnthropicAdapter {
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
        self.core.models(credential, transport, "x-api-key")
    }

    fn model_headers(
        &self,
        credential: &Credential,
    ) -> Result<Vec<(String, String)>, AuthProviderError> {
        self.core.model_headers(credential, "x-api-key")
    }
}
