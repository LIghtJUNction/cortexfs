use super::common::AdapterCore;
use super::protocol::parse_models;
use super::{AuthProviderError, AuthTransport, Credential};

impl AdapterCore {
    pub fn model_headers(
        &self,
        credential: &Credential,
        api_key_header: &str,
    ) -> Result<Vec<(String, String)>, AuthProviderError> {
        let (name, value) = match *credential {
            Credential::ApiKey {
                ref provider,
                ref key,
                ..
            } if provider == &self.id => {
                if invalid_header_value(key) {
                    return Err(AuthProviderError::InvalidCredential);
                }
                let value = if api_key_header == "Authorization" {
                    format!("Bearer {key}")
                } else {
                    key.clone()
                };
                (api_key_header.to_owned(), value)
            }
            Credential::OAuth {
                ref provider,
                ref access_token,
                ..
            } if provider == &self.id => {
                if invalid_header_value(access_token) {
                    return Err(AuthProviderError::InvalidCredential);
                }
                ("Authorization".to_owned(), format!("Bearer {access_token}"))
            }
            _ => return Err(AuthProviderError::InvalidCredential),
        };
        Ok(vec![(name, value)])
    }

    pub fn models(
        &self,
        credential: Option<&Credential>,
        transport: &mut dyn AuthTransport,
        api_key_header: &str,
    ) -> Result<Vec<String>, AuthProviderError> {
        let credential = credential.ok_or(AuthProviderError::InvalidCredential)?;
        let headers = self.model_headers(credential, api_key_header)?;
        let headers = headers
            .iter()
            .map(|header| (header.0.as_str(), header.1.as_str()))
            .collect::<Vec<_>>();
        let response = transport.get(&self.model_url, &headers)?;
        parse_models(&response)
    }
}

fn invalid_header_value(value: &str) -> bool {
    value.trim().is_empty() || value.bytes().any(|byte| byte.is_ascii_control())
}

pub fn model_url(base_url: &str) -> String {
    format!("{}/models", base_url.trim_end_matches('/'))
}
