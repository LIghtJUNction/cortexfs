use cortexfs_channels::OutboundRequest;
use reqwest::blocking::Client;
use serde_json::Value;

use super::{MochatConfig, MochatError};

pub(super) fn receive(
    client: &Client,
    config: &MochatConfig,
    since_id: Option<&str>,
) -> Result<String, MochatError> {
    let mut request = client
        .get(url(&config.api_base, "api/message/receive"))
        .bearer_auth(&config.api_token);
    if let Some(since_id) = since_id {
        request = request.query(&[("since_id", since_id)]);
    }
    request
        .send()
        .map_err(MochatError::Http)?
        .error_for_status()
        .map_err(MochatError::Http)?
        .text()
        .map_err(MochatError::Http)
}

pub(super) fn send(
    client: &Client,
    config: &MochatConfig,
    request: OutboundRequest,
) -> Result<(), MochatError> {
    let mut builder = client
        .post(url(&config.api_base, &request.path))
        .bearer_auth(&config.api_token)
        .header(reqwest::header::CONTENT_TYPE, request.content_type)
        .body(request.body);
    for (name, value) in request.headers {
        builder = builder.header(name, value);
    }
    let value = builder
        .send()
        .map_err(MochatError::Http)?
        .error_for_status()
        .map_err(MochatError::Http)?
        .json::<Value>()
        .map_err(MochatError::Http)?;
    let code = value.get("code").and_then(Value::as_i64).unwrap_or(0);
    if code != 0 && code != 200 {
        return Err(MochatError::Protocol(format!("Mochat API code {code}")));
    }
    Ok(())
}

fn url(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}
