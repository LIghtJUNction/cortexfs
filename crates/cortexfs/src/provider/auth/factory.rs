use super::registry::ProviderRegistry;
use super::{AuthProvider, ProviderAuthConfig};
use crate::provider::oauth::OAuthProviderConfig;

/// Selects the concrete adapter for a host provider declaration.
#[must_use]
pub fn configured_adapter(
    id: &str,
    base_url: &str,
    methods: Vec<ProviderAuthConfig>,
    oauth: Option<OAuthProviderConfig>,
) -> Option<Box<dyn AuthProvider>> {
    match id {
        "anthropic" | "claude" => Some(Box::new(super::anthropic::AnthropicAdapter::new(
            id, base_url, methods, oauth,
        ))),
        "github-copilot" | "copilot" => oauth.map(|config| {
            let adapter: Box<dyn AuthProvider> = Box::new(
                super::copilot::GitHubCopilotAdapter::with_config(id, base_url, methods, config),
            );
            adapter
        }),
        _ => Some(Box::new(super::openai::OpenAiAdapter::new(
            id, base_url, methods, oauth,
        ))),
    }
}

/// Builds a registry containing the adapter selected by host metadata.
#[must_use]
pub fn configured_registry(
    id: &str,
    base_url: &str,
    methods: Vec<ProviderAuthConfig>,
    oauth: Option<OAuthProviderConfig>,
) -> Option<ProviderRegistry> {
    let adapter = configured_adapter(id, base_url, methods, oauth)?;
    let mut registry = ProviderRegistry::new();
    registry.register_boxed(adapter).ok()?;
    Some(registry)
}
