use base64::{Engine as _, engine::general_purpose::STANDARD};
use cortexfs_channels::MessageTarget;
use reqwest::blocking::{Client, multipart};
use serde_json::Value;

use super::super::{MattermostConfig, MattermostError, api};

pub(super) fn run(
    client: &Client,
    config: &MattermostConfig,
    target: &MessageTarget,
    payload: &Value,
) -> Result<Value, MattermostError> {
    let data = STANDARD
        .decode(
            payload
                .get("data_base64")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or(MattermostError::Protocol(
                    "data_base64 is missing".to_owned(),
                ))?,
        )
        .map_err(|error| MattermostError::Protocol(format!("invalid file: {error}")))?;
    if data.len() > 128 * 1024 {
        return Err(MattermostError::Protocol("file is too large".to_owned()));
    }
    let filename = payload
        .get("filename")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(MattermostError::Protocol("filename is missing".to_owned()))?;
    let form = multipart::Form::new()
        .text("channel_id", target.conversation.to_string())
        .part(
            "files",
            multipart::Part::bytes(data).file_name(filename.to_owned()),
        );
    client
        .post(api::endpoint(config, "api/v4/files"))
        .bearer_auth(&config.token)
        .multipart(form)
        .send()
        .map_err(MattermostError::Http)?
        .error_for_status()
        .map_err(MattermostError::Http)?
        .json::<Value>()
        .map_err(MattermostError::Http)
}
