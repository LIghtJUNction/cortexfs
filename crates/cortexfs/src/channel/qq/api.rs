use cortexfs_channels::OutboundRequest;
use reqwest::blocking::Client;
use serde_json::Value;

use super::{QqConfig, QqError};

pub(super) fn gateway(client: &Client, config: &QqConfig) -> Result<String, QqError> {
    client
        .get(&config.gateway_url)
        .header(reqwest::header::AUTHORIZATION, config.auth())
        .send()
        .map_err(QqError::Http)?
        .error_for_status()
        .map_err(QqError::Http)?
        .json::<Value>()
        .map_err(QqError::Http)?
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| QqError::Protocol("gateway URL is missing".to_owned()))
}

pub(super) fn send(
    client: &Client,
    config: &QqConfig,
    request: OutboundRequest,
) -> Result<(), QqError> {
    client
        .post(format!(
            "{}/{}",
            config.api_base,
            request.path.trim_start_matches('/')
        ))
        .header(reqwest::header::AUTHORIZATION, config.auth())
        .header(reqwest::header::CONTENT_TYPE, request.content_type)
        .body(request.body)
        .send()
        .map_err(QqError::Http)?
        .error_for_status()
        .map_err(QqError::Http)?;
    Ok(())
}
