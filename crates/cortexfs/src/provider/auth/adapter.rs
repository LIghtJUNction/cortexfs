use super::{Credential, ProviderAuthConfig};

/// Input supplied to a provider adapter's login operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthRequest {
    /// A key supplied by the host-side secret command.
    ApiKey { slot: String, key: String },
    /// Authorization code returned to the local callback.
    AuthorizationCode { code: String },
    /// Device flow with a bounded polling budget.
    DeviceCode { timeout_secs: u64 },
}

/// Stable failures returned by provider authentication adapters.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum AuthProviderError {
    /// The adapter does not implement the requested flow.
    #[error("unsupported authentication method")]
    UnsupportedMethod,
    /// The credential is malformed or cannot be refreshed.
    #[error("invalid provider credential")]
    InvalidCredential,
    /// The provider or its local credential store was unavailable.
    #[error("provider authentication unavailable")]
    Unavailable,
}

/// Provider adapter boundary shared by OAuth, API-key, and future providers.
pub trait AuthProvider {
    /// Stable provider identity used by model routes and secret slots.
    fn id(&self) -> &str;
    /// Authentication methods supported by this adapter.
    fn methods(&self) -> &[ProviderAuthConfig];
    /// Starts a login and returns a normalized credential.
    fn login(&self, request: AuthRequest) -> Result<Credential, AuthProviderError>;
    /// Refreshes a normalized OAuth credential when supported.
    fn refresh(&self, credential: &Credential) -> Result<Credential, AuthProviderError>;
    /// Lists provider model identifiers without exposing provider-native API data.
    fn models(&self, credential: Option<&Credential>) -> Result<Vec<String>, AuthProviderError>;
}
