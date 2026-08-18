use cortexfs_channels::OutboundRequest;
use reqwest::blocking::Client;
use serde_json::Value;

use super::{MatrixConfig, MatrixError};

pub(super) fn whoami(client: &Client, config: &MatrixConfig) -> Result<String, MatrixError> {
    client
        .get(endpoint(config, "account/whoami"))
        .bearer_auth(&config.access_token)
        .send()
        .map_err(MatrixError::Http)?
        .error_for_status()
        .map_err(MatrixError::Http)?
        .json::<Value>()
        .map_err(MatrixError::Http)?
        .get("user_id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| MatrixError::Protocol("whoami user_id is missing".to_owned()))
}

pub(super) fn sync(
    client: &Client,
    config: &MatrixConfig,
    since: Option<&str>,
) -> Result<Value, MatrixError> {
    let mut url = url::Url::parse(&endpoint(config, "sync"))
        .map_err(|error| MatrixError::Config(error.to_string()))?;
    url.query_pairs_mut()
        .append_pair("timeout", &config.timeout().as_millis().to_string());
    if let Some(since) = since {
        url.query_pairs_mut().append_pair("since", since);
    }
    client
        .get(url)
        .bearer_auth(&config.access_token)
        .send()
        .map_err(MatrixError::Http)?
        .error_for_status()
        .map_err(MatrixError::Http)?
        .json::<Value>()
        .map_err(MatrixError::Http)
}

pub(super) fn send(
    client: &Client,
    config: &MatrixConfig,
    request: OutboundRequest,
    transaction: &str,
) -> Result<(), MatrixError> {
    let url = format!(
        "{}{}/{}",
        endpoint(config, ""),
        request.path.trim_start_matches('/'),
        transaction
    );
    let builder = match request.method.to_ascii_uppercase().as_str() {
        "POST" => client.post(url),
        "PUT" => client.put(url),
        "PATCH" => client.patch(url),
        "DELETE" => client.delete(url),
        method => {
            return Err(MatrixError::Protocol(format!(
                "unsupported Matrix outbound method: {method}"
            )));
        }
    };
    builder
        .bearer_auth(&config.access_token)
        .header(reqwest::header::CONTENT_TYPE, request.content_type)
        .body(request.body)
        .send()
        .map_err(MatrixError::Http)?
        .error_for_status()
        .map_err(MatrixError::Http)?;
    Ok(())
}

fn endpoint(config: &MatrixConfig, path: &str) -> String {
    format!("{}/_matrix/client/v3/{path}", config.homeserver)
}
