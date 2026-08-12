use super::{Credential, ProviderAuthConfig};
use crate::provider::oauth::OAuthPkce;

use super::protocol::unix_time;
use super::transport::HttpTransport;

/// Input supplied to a provider adapter's login operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AuthRequest {
    /// A key supplied by the host-side secret command.
    ApiKey { slot: String, key: String },
    /// Authorization code returned to the local callback.
    AuthorizationCode { code: String },
    /// Authorization code paired with the PKCE verifier held by the host.
    AuthorizationCodePkce { code: String, verifier: String },
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
    /// Provider configuration is incomplete or unsafe.
    #[error("invalid provider configuration")]
    InvalidConfig,
    /// The provider returned a response outside the normalized contract.
    #[error("invalid provider response")]
    InvalidResponse,
    /// The provider or its local credential store was unavailable.
    #[error("provider authentication unavailable")]
    Unavailable,
}

/// Provider adapter boundary shared by OAuth, API-key, and future providers.
pub trait AuthProvider: Send + Sync {
    /// Stable provider identity used by model routes and secret slots.
    fn id(&self) -> &str;
    /// Authentication methods supported by this adapter.
    fn methods(&self) -> &[ProviderAuthConfig];
    /// Optional aliases accepted by the host-side registry.
    fn aliases(&self) -> &[String];
    /// Builds the provider's authorization URL for an interactive login.
    fn authorization_url(&self, state: &str, pkce: &OAuthPkce)
    -> Result<String, AuthProviderError>;
    /// Starts a login and returns a normalized credential.
    fn login(&self, request: AuthRequest) -> Result<Credential, AuthProviderError>;
    /// Starts a login with an injected transport and clock.
    fn login_with(
        &self,
        request: AuthRequest,
        transport: &mut dyn AuthTransport,
        now: u64,
    ) -> Result<Credential, AuthProviderError>;
    /// Refreshes a normalized OAuth credential when supported.
    fn refresh(&self, credential: &Credential) -> Result<Credential, AuthProviderError>;
    /// Refreshes a credential with an injected transport and clock.
    fn refresh_with(
        &self,
        credential: &Credential,
        transport: &mut dyn AuthTransport,
        now: u64,
    ) -> Result<Credential, AuthProviderError>;
    /// Lists provider model identifiers without exposing provider-native API data.
    fn models(&self, credential: Option<&Credential>) -> Result<Vec<String>, AuthProviderError>;
    /// Discovers models with an injected transport.
    fn models_with(
        &self,
        credential: Option<&Credential>,
        transport: &mut dyn AuthTransport,
    ) -> Result<Vec<String>, AuthProviderError>;
    /// Returns provider-specific headers for the hardened host discovery path.
    fn model_headers(
        &self,
        credential: &Credential,
    ) -> Result<Vec<(String, String)>, AuthProviderError>;
    /// Parses a bounded model response returned by the host discovery path.
    fn parse_models(&self, response: AuthResponse) -> Result<Vec<String>, AuthProviderError> {
        super::protocol::parse_models(&response)
    }
}

/// Bounded HTTP response passed to provider adapters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response body, capped by the transport implementation.
    pub body: Vec<u8>,
}

/// Transport boundary used by adapters and deterministic tests.
pub trait AuthTransport {
    /// Sends a form or JSON POST request.
    fn post(
        &mut self,
        url: &str,
        content_type: &str,
        body: &str,
    ) -> Result<AuthResponse, AuthProviderError>;
    /// Sends a GET request with provider-specific headers.
    fn get(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<AuthResponse, AuthProviderError>;
}

/// Uses the existing blocking HTTP stack for host-side adapter calls.
pub fn http_transport() -> Result<impl AuthTransport, AuthProviderError> {
    HttpTransport::new().map_err(|_error| AuthProviderError::Unavailable)
}

/// Runs a login through the default transport and current Unix time.
pub fn default_login(
    provider: &dyn AuthProvider,
    request: AuthRequest,
) -> Result<Credential, AuthProviderError> {
    let mut transport = http_transport()?;
    provider.login_with(request, &mut transport, unix_time())
}

/// Runs a refresh through the default transport and current Unix time.
pub fn default_refresh(
    provider: &dyn AuthProvider,
    credential: &Credential,
) -> Result<Credential, AuthProviderError> {
    let mut transport = http_transport()?;
    provider.refresh_with(credential, &mut transport, unix_time())
}

/// Runs model discovery through the default transport.
pub fn default_models(
    provider: &dyn AuthProvider,
    credential: Option<&Credential>,
) -> Result<Vec<String>, AuthProviderError> {
    let mut transport = http_transport()?;
    provider.models_with(credential, &mut transport)
}
