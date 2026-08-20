mod adapter;
pub mod anthropic;
mod codexdevice;
mod common;
pub mod copilot;
mod credential;
mod device;
mod deviceparse;
mod factory;
mod model;
pub mod openai;
mod persist;
mod profile;
mod protocol;
mod registry;
mod transport;
mod wire;

pub use adapter::{
    AuthProvider, AuthProviderError, AuthRequest, AuthResponse, AuthTransport, DeviceChallenge,
    default_device_login, device_request, http_transport, refresh_oauth_result,
};
pub use credential::{Credential, CredentialKind};
pub use factory::{configured_adapter, configured_registry};
pub use profile::{AuthProfile, AuthProfileError, read_auth_profile, store_auth_profile};
pub use registry::{ProviderRegistry, ProviderRegistryError};
use serde::{Deserialize, Serialize};
pub use wire::{AUTH_SOCKET_ABI, AuthWireError, AuthWireFrame, AuthWireRequest, AuthWireResponse};

/// Authentication mechanism advertised by a provider.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    /// A key stored in the provider secret slot.
    ApiKey,
    /// OAuth with a browser callback or a device grant.
    OAuth,
}

/// OAuth grant used by a provider adapter.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthFlow {
    /// Authorization Code + PKCE with a local callback.
    #[default]
    AuthorizationCode,
    /// Device authorization for headless clients.
    DeviceCode,
}

/// Provider authentication declaration from host-side provider JSON.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProviderAuthConfig {
    /// Authentication mechanism.
    #[serde(rename = "type")]
    pub method: AuthMethod,
    /// OAuth grant, when `method` is [`AuthMethod::OAuth`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flow: Option<OAuthFlow>,
    /// Logical credential slot; token-store account names remain internal.
    #[serde(default = "default_slot")]
    pub slot: String,
}

impl ProviderAuthConfig {
    /// Creates an API-key declaration for a logical slot.
    #[must_use]
    pub fn api_key(slot: impl Into<String>) -> Self {
        Self {
            method: AuthMethod::ApiKey,
            flow: None,
            slot: slot.into(),
        }
    }

    /// Creates an OAuth declaration for a logical slot.
    #[must_use]
    pub fn oauth(flow: OAuthFlow, slot: impl Into<String>) -> Self {
        Self {
            method: AuthMethod::OAuth,
            flow: Some(flow),
            slot: slot.into(),
        }
    }

    /// Returns whether the declaration is structurally valid.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        crate::is_object_name(&self.slot)
            && match self.method {
                AuthMethod::ApiKey => self.flow.is_none(),
                AuthMethod::OAuth => self.flow.is_some(),
            }
    }
}

/// Applies the compatibility default for provider JSON without `auth`.
#[must_use]
pub fn effective_auth_methods(
    explicit: &[ProviderAuthConfig],
    has_legacy_oauth: bool,
) -> Vec<ProviderAuthConfig> {
    if !explicit.is_empty() {
        return explicit.to_vec();
    }
    let mut methods = vec![ProviderAuthConfig::api_key("default")];
    if has_legacy_oauth {
        methods.push(ProviderAuthConfig::oauth(
            OAuthFlow::AuthorizationCode,
            "oauth",
        ));
    }
    methods
}

fn default_slot() -> String {
    "default".to_owned()
}
