use super::adapter::AuthProviderError;
use super::common::{AdapterCore, CoreAuthProvider};
use super::device::DeviceConfig;
use super::model::model_url;
use super::{Credential, ProviderAuthConfig};
use crate::provider::oauth::OAuthProviderConfig;
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
        let aliases = match id.as_str() {
            "anthropic" => vec!["claude".to_owned()],
            "claude" => vec!["anthropic".to_owned()],
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
impl CoreAuthProvider for AnthropicAdapter {
    fn core(&self) -> &AdapterCore {
        &self.core
    }
    fn headers(&self, credential: &Credential) -> Result<Vec<(String, String)>, AuthProviderError> {
        let mut headers = self.core.model_headers(credential, "x-api-key")?;
        headers.push(("anthropic-version".to_owned(), "2023-06-01".to_owned()));
        Ok(headers)
    }
}
