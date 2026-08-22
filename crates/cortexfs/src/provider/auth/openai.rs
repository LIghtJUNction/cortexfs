use super::adapter::{AuthProviderError, AuthTransport};
use super::codexdevice;
use super::common::{AdapterCore, CoreAuthProvider};
use super::device::DeviceChallenge;
use super::{Credential, ProviderAuthConfig};
use crate::provider::oauth::{OAuthProviderConfig, codex_oauth_config};
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
        Self {
            core: AdapterCore::configured(id, base_url, methods, oauth, ["codex", "openai"]),
        }
    }
}
impl CoreAuthProvider for OpenAiAdapter {
    fn core(&self) -> &AdapterCore {
        &self.core
    }

    fn device_login(
        &self,
        timeout_secs: u64,
        transport: &mut dyn AuthTransport,
        now: u64,
        notify: &mut dyn FnMut(&DeviceChallenge),
        pause: &mut dyn FnMut(u64),
    ) -> Result<Credential, AuthProviderError> {
        if self
            .core
            .oauth
            .as_ref()
            .is_some_and(OAuthProviderConfig::is_codex)
        {
            return codexdevice::login(&self.core.id, timeout_secs, transport, now, notify, pause);
        }
        self.core
            .device_login(timeout_secs, transport, now, notify, pause)
    }
}
