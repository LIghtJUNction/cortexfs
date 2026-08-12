use super::device::{self, DeviceChallenge, DeviceConfig};
use super::protocol::{credential_from_token, parse_token};
use super::{AuthProviderError, AuthRequest, AuthTransport, Credential, ProviderAuthConfig};
use crate::provider::oauth::{
    OAuthPkce, OAuthProviderConfig, oauth_authorization_code_form, oauth_authorization_url,
    oauth_refresh_token_form,
};
#[derive(Debug)]
pub struct AdapterCore {
    pub(crate) id: String,
    pub(crate) aliases: Vec<String>,
    pub(crate) model_url: String,
    pub(crate) methods: Vec<ProviderAuthConfig>,
    pub(crate) oauth: Option<OAuthProviderConfig>,
    pub(crate) device: Option<DeviceConfig>,
}
impl AdapterCore {
    pub(crate) fn authorization(
        &self,
        state: &str,
        pkce: &OAuthPkce,
    ) -> Result<String, AuthProviderError> {
        if !self.methods.iter().any(|method| {
            method.method == super::AuthMethod::OAuth
                && method.flow == Some(super::OAuthFlow::AuthorizationCode)
        }) {
            return Err(AuthProviderError::UnsupportedMethod);
        }
        let config = self
            .oauth
            .as_ref()
            .ok_or(AuthProviderError::UnsupportedMethod)?;
        oauth_authorization_url(config, state, pkce)
            .map_err(|_error| AuthProviderError::InvalidConfig)
    }
    pub(crate) fn login(
        &self,
        request: AuthRequest,
        transport: &mut dyn AuthTransport,
        now: u64,
    ) -> Result<Credential, AuthProviderError> {
        match request {
            AuthRequest::ApiKey { slot, key } => self.api_key(&slot, key),
            AuthRequest::AuthorizationCodePkce { code, verifier } => {
                self.exchange(&code, &verifier, transport, now)
            }
            AuthRequest::AuthorizationCode { .. } => Err(AuthProviderError::InvalidCredential),
            AuthRequest::DeviceCode { .. } => Err(AuthProviderError::UnsupportedMethod),
        }
    }
    pub(crate) fn device_login(
        &self,
        timeout_secs: u64,
        transport: &mut dyn AuthTransport,
        now: u64,
        notify: &mut dyn FnMut(&DeviceChallenge),
        pause: &mut dyn FnMut(u64),
    ) -> Result<Credential, AuthProviderError> {
        if !self.methods.iter().any(|method| {
            method.method == super::AuthMethod::OAuth
                && method.flow == Some(super::OAuthFlow::DeviceCode)
        }) {
            return Err(AuthProviderError::UnsupportedMethod);
        }
        let oauth = self
            .oauth
            .as_ref()
            .ok_or(AuthProviderError::InvalidConfig)?;
        let device = self
            .device
            .as_ref()
            .ok_or(AuthProviderError::UnsupportedMethod)?;
        device::login(
            &self.id,
            oauth,
            device,
            timeout_secs,
            transport,
            now,
            notify,
            pause,
        )
    }
    pub(crate) fn persist(
        &self,
        credential: &Credential,
        now: u64,
    ) -> Result<(), AuthProviderError> {
        super::persist::store(self, credential, now)
    }
    pub(crate) fn refresh(
        &self,
        credential: &Credential,
        transport: &mut dyn AuthTransport,
        now: u64,
    ) -> Result<Credential, AuthProviderError> {
        if !self
            .methods
            .iter()
            .any(|method| method.method == super::AuthMethod::OAuth)
        {
            return Err(AuthProviderError::UnsupportedMethod);
        }
        let Credential::OAuth {
            ref provider,
            refresh_token: Some(ref refresh),
            ..
        } = *credential
        else {
            return Err(AuthProviderError::InvalidCredential);
        };
        if provider.as_str() != self.id.as_str() || refresh.trim().is_empty() {
            return Err(AuthProviderError::InvalidCredential);
        }
        let config = self
            .oauth
            .as_ref()
            .ok_or(AuthProviderError::UnsupportedMethod)?;
        let form = oauth_refresh_token_form(config, refresh)
            .map_err(|_error| AuthProviderError::InvalidConfig)?;
        let response = transport.post_with_headers(
            &config.token_url,
            "application/x-www-form-urlencoded",
            &form,
            &[("Accept", "application/json")],
        )?;
        let token = parse_token(&response)?;
        credential_from_token(&self.id, token, Some(credential), now)
    }
    fn api_key(&self, slot: &str, key: String) -> Result<Credential, AuthProviderError> {
        let valid = self
            .methods
            .iter()
            .any(|method| method.method == super::AuthMethod::ApiKey && method.slot == slot);
        if !valid || key.trim().is_empty() || key.bytes().any(|byte| byte.is_ascii_control()) {
            return Err(AuthProviderError::InvalidCredential);
        }
        Ok(Credential::ApiKey {
            provider: self.id.clone(),
            key,
            slot: Some(slot.to_owned()),
        })
    }
    fn exchange(
        &self,
        code: &str,
        verifier: &str,
        transport: &mut dyn AuthTransport,
        now: u64,
    ) -> Result<Credential, AuthProviderError> {
        let supported = self.methods.iter().any(|method| {
            method.method == super::AuthMethod::OAuth
                && method.flow == Some(super::OAuthFlow::AuthorizationCode)
        });
        if !supported {
            return Err(AuthProviderError::UnsupportedMethod);
        }
        let config = self
            .oauth
            .as_ref()
            .ok_or(AuthProviderError::InvalidConfig)?;
        let pkce = OAuthPkce::from_verifier(verifier)
            .map_err(|_error| AuthProviderError::InvalidCredential)?;
        let form = oauth_authorization_code_form(config, code, &pkce)
            .map_err(|_error| AuthProviderError::InvalidConfig)?;
        let response = transport.post_with_headers(
            &config.token_url,
            "application/x-www-form-urlencoded",
            &form,
            &[("Accept", "application/json")],
        )?;
        credential_from_token(&self.id, parse_token(&response)?, None, now)
    }
}
