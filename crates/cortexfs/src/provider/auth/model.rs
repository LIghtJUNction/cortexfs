use super::common::AdapterCore;
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

    pub fn model_response_with_headers(
        &self,
        transport: &mut dyn AuthTransport,
        headers: &[(String, String)],
    ) -> Result<super::AuthResponse, AuthProviderError> {
        let headers = headers
            .iter()
            .map(|header| (header.0.as_str(), header.1.as_str()))
            .collect::<Vec<_>>();
        transport.get(&self.model_url, &headers)
    }
}

fn invalid_header_value(value: &str) -> bool {
    value.trim().is_empty() || super::has_ascii_control(value)
}

pub fn model_url(base_url: &str) -> String {
    format!("{}/models", base_url.trim_end_matches('/'))
}
