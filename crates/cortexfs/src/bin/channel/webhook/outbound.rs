use cortexfs_channels::OutboundRequest;

use super::{WebhookConfig, signature};
use crate::config::Platform;

#[derive(Debug, thiserror::Error)]
pub(super) enum OutboundError {
    #[error("platform HTTP request failed")]
    Http(#[source] reqwest::Error),
    #[error(transparent)]
    Signature(#[from] signature::SignatureError),
    #[error("unsupported platform HTTP method: {0}")]
    Method(String),
}

pub(super) fn send(
    client: &reqwest::blocking::Client,
    config: &WebhookConfig,
    request: OutboundRequest,
) -> Result<(), OutboundError> {
    let url = config.outbound_url.replace("{path}", &request.path);
    let body = request.body;
    let mut builder = match request.method.as_str() {
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "PATCH" => client.patch(url),
        "DELETE" => client.delete(url),
        method => return Err(OutboundError::Method(method.to_owned())),
    }
    .header(reqwest::header::CONTENT_TYPE, request.content_type)
    .body(body.clone());
    if matches!(config.platform, Platform::Nextcloud) {
        let secret = config
            .token
            .as_deref()
            .ok_or(signature::SignatureError::MissingSecret)?;
        let random = signature::nonce()?;
        let digest = signature::hmac_hex(secret, &format!("{random}{body}"));
        builder = builder
            .header("X-Nextcloud-Talk-Bot-Random", random)
            .header("X-Nextcloud-Talk-Bot-Signature", digest)
            .header("OCS-APIRequest", "true");
    } else if let Some(token) = config.token.as_deref() {
        builder = builder.bearer_auth(token);
    }
    for (name, value) in request.headers {
        builder = builder.header(name, value);
    }
    builder
        .send()
        .map_err(OutboundError::Http)?
        .error_for_status()
        .map_err(OutboundError::Http)?;
    Ok(())
}
