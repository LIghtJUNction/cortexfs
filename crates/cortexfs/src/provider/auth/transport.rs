use super::{AuthProviderError, AuthResponse, AuthTransport};
use reqwest::blocking::Client;
use std::time::Duration;

const MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// Small blocking transport shared by host-side provider adapters.
#[derive(Debug)]
pub struct HttpTransport {
    client: Client,
}

impl HttpTransport {
    pub(crate) fn new() -> Result<Self, reqwest::Error> {
        Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map(|client| Self { client })
    }

    fn send(request: reqwest::blocking::RequestBuilder) -> Result<AuthResponse, AuthProviderError> {
        let response = request
            .send()
            .map_err(|_error| AuthProviderError::Unavailable)?;
        let status = response.status().as_u16();
        let body = crate::support::process::read_limited_bytes(response, MAX_RESPONSE_BYTES + 1);
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(AuthProviderError::InvalidResponse);
        }
        Ok(AuthResponse { status, body })
    }
}

impl AuthTransport for HttpTransport {
    fn post(
        &mut self,
        url: &str,
        content_type: &str,
        body: &str,
    ) -> Result<AuthResponse, AuthProviderError> {
        self.post_with_headers(url, content_type, body, &[])
    }

    fn post_with_headers(
        &mut self,
        url: &str,
        content_type: &str,
        body: &str,
        headers: &[(&str, &str)],
    ) -> Result<AuthResponse, AuthProviderError> {
        let request = headers.iter().fold(
            self.client
                .post(url)
                .header(reqwest::header::CONTENT_TYPE, content_type),
            |request, &(name, value)| request.header(name, value),
        );
        Self::send(request.body(body.to_owned()))
    }

    fn get(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<AuthResponse, AuthProviderError> {
        let request = headers
            .iter()
            .fold(self.client.get(url), |request, header| {
                let &(name, value) = header;
                request.header(name, value)
            });
        Self::send(request)
    }
}
