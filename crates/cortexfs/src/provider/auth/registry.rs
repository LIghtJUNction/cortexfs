use super::{AuthProvider, AuthProviderError};
use crate::provider::oauth::OAuthProviderConfig;
use std::fmt;

/// Registry failures are configuration errors, never provider credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProviderRegistryError {
    /// The provider id or alias is not a safe object name.
    #[error("invalid provider registry name")]
    InvalidName,
    /// A provider id or alias is already registered.
    #[error("duplicate provider registry name")]
    DuplicateName,
}

/// Runtime registry that resolves provider identity before auth or model calls.
pub struct ProviderRegistry {
    providers: Vec<Box<dyn AuthProvider>>,
}

impl fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderRegistry")
            .field("providers", &self.names())
            .finish()
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    /// Creates an empty registry for host-configured adapters.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    /// Registers one concrete provider adapter.
    pub fn register<P: AuthProvider + 'static>(
        &mut self,
        provider: P,
    ) -> Result<(), ProviderRegistryError> {
        if !crate::is_object_name(provider.id()) || self.find(provider.id()).is_some() {
            return Err(if crate::is_object_name(provider.id()) {
                ProviderRegistryError::DuplicateName
            } else {
                ProviderRegistryError::InvalidName
            });
        }
        for name in provider.aliases().iter().map(String::as_str) {
            if name == provider.id() {
                continue;
            }
            if !crate::is_object_name(name) {
                return Err(ProviderRegistryError::InvalidName);
            }
            if self.find(name).is_some() {
                return Err(ProviderRegistryError::DuplicateName);
            }
        }
        self.providers.push(Box::new(provider));
        Ok(())
    }

    /// Resolves a provider by canonical id or adapter alias.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&dyn AuthProvider> {
        self.providers
            .iter()
            .find(|provider| {
                provider.id() == name || provider.aliases().iter().any(|alias| alias == name)
            })
            .map(Box::as_ref)
    }

    /// Returns canonical ids in registration order.
    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.providers
            .iter()
            .map(|provider| provider.id().to_owned())
            .collect()
    }

    /// Adds the built-in `OpenAI` and Claude profiles and optional Copilot config.
    pub fn with_defaults(
        copilot: Option<OAuthProviderConfig>,
    ) -> Result<Self, ProviderRegistryError> {
        let mut registry = Self::new();
        registry.register(super::openai::OpenAiAdapter::codex())?;
        registry.register(super::anthropic::AnthropicAdapter::claude())?;
        if let Some(config) = copilot {
            registry.register(super::copilot::GitHubCopilotAdapter::new(config))?;
        }
        Ok(registry)
    }

    fn find(&self, name: &str) -> Option<&dyn AuthProvider> {
        self.get(name)
    }
}

impl From<ProviderRegistryError> for AuthProviderError {
    fn from(_error: ProviderRegistryError) -> Self {
        Self::InvalidConfig
    }
}
