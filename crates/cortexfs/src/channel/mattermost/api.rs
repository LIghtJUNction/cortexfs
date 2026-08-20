use cortexfs_channels::OutboundRequest;
use reqwest::blocking::Client;
use serde_json::Value;

use super::{MattermostConfig, MattermostError};

pub(super) fn current_user(
    client: &Client,
    config: &MattermostConfig,
) -> Result<String, MattermostError> {
    client
        .get(endpoint(config, "api/v4/users/me"))
        .bearer_auth(&config.token)
        .send()
        .map_err(MattermostError::Http)?
        .error_for_status()
        .map_err(MattermostError::Http)?
        .json::<Value>()
        .map_err(MattermostError::Http)?
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| MattermostError::Protocol("current user id is missing".to_owned()))
}

pub(super) fn send(
    client: &Client,
    config: &MattermostConfig,
    request: OutboundRequest,
) -> Result<(), MattermostError> {
    let builder = match request.method.as_str() {
        "DELETE" => client.delete(endpoint(config, &request.path)),
        "POST" => client.post(endpoint(config, &request.path)),
        "PUT" => client.put(endpoint(config, &request.path)),
        method => {
            return Err(MattermostError::Protocol(format!(
                "unsupported Mattermost method {method}"
            )));
        }
    };
    builder
        .bearer_auth(&config.token)
        .header(reqwest::header::CONTENT_TYPE, request.content_type)
        .body(request.body)
        .send()
        .map_err(MattermostError::Http)?
        .error_for_status()
        .map_err(MattermostError::Http)?;
    Ok(())
}

pub(super) fn endpoint(config: &MattermostConfig, path: &str) -> String {
    format!("{}/{}", config.base_url, path.trim_start_matches('/'))
}
