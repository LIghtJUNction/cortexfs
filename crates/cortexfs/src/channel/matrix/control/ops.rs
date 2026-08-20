use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::blocking::Client;
use serde_json::Value;

use super::super::{MatrixConfig, MatrixError, api};

pub(super) fn upload(
    client: &Client,
    config: &MatrixConfig,
    payload: &Value,
) -> Result<Value, MatrixError> {
    let encoded = string(payload, "data_base64")?;
    let data = STANDARD
        .decode(encoded)
        .map_err(|error| MatrixError::Protocol(format!("invalid media: {error}")))?;
    if data.len() > 128 * 1024 {
        return Err(MatrixError::Protocol("media is too large".to_owned()));
    }
    let content_type = payload
        .get("content_type")
        .and_then(Value::as_str)
        .unwrap_or("application/octet-stream");
    client
        .post(format!("{}/_matrix/media/v3/upload", config.homeserver))
        .bearer_auth(&config.access_token)
        .header(reqwest::header::CONTENT_TYPE, content_type)
        .body(data)
        .send()
        .map_err(MatrixError::Http)?
        .error_for_status()
        .map_err(MatrixError::Http)?
        .json::<Value>()
        .map_err(MatrixError::Http)
}

pub(super) fn call(
    client: &Client,
    config: &MatrixConfig,
    method: &str,
    path: &str,
    body: &Value,
) -> Result<Value, MatrixError> {
    let request = match method {
        "POST" => client.post(api::endpoint(config, path)),
        "PUT" => client.put(api::endpoint(config, path)),
        _ => return Err(MatrixError::Protocol("unsupported method".to_owned())),
    };
    request
        .bearer_auth(&config.access_token)
        .json(body)
        .send()
        .map_err(MatrixError::Http)?
        .error_for_status()
        .map_err(MatrixError::Http)?
        .json::<Value>()
        .map_err(MatrixError::Http)
}

pub(super) fn string<'a>(value: &'a Value, name: &'static str) -> Result<&'a str, MatrixError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or(MatrixError::Protocol(format!("{name} is missing")))
}

pub(super) struct MissingRoom;
impl From<MissingRoom> for MatrixError {
    fn from(_: MissingRoom) -> Self {
        Self::Protocol("room is missing".to_owned())
    }
}
