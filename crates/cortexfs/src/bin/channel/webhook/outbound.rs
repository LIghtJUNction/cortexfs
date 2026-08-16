use cortexfs_channels::OutboundRequest;

use super::WebhookConfig;

#[derive(Debug, thiserror::Error)]
pub(super) enum OutboundError {
    #[error("platform HTTP request failed")]
    Http(#[source] reqwest::Error),
}

pub(super) fn send(
    client: &reqwest::blocking::Client,
    config: &WebhookConfig,
    request: OutboundRequest,
) -> Result<(), OutboundError> {
    let url = config.outbound_url.replace("{path}", &request.path);
    let mut builder = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, request.content_type)
        .body(request.body);
    if let Some(token) = config.token.as_deref() {
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
