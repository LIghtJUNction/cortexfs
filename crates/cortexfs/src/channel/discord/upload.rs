use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::blocking::{Client, multipart};
use serde_json::{Value, json};

use super::{DiscordConfig, DiscordError, effect, embed, request};

const MAX_ENCODED_BYTES: usize = 192 * 1024;
const MAX_FILE_BYTES: usize = 128 * 1024;

pub(super) fn send(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    command_id: &str,
    payload: &Value,
) -> Result<Value, DiscordError> {
    let filename = string(payload, "filename")?;
    if filename.len() > 128 || filename.contains(['/', '\\']) {
        return Err(DiscordError::Invalid("filename"));
    }
    let encoded = string(payload, "data_base64")?;
    if encoded.len() > MAX_ENCODED_BYTES {
        return Err(DiscordError::Invalid("data_base64"));
    }
    let data = STANDARD
        .decode(encoded)
        .map_err(|_error| DiscordError::Invalid("data_base64"))?;
    if data.len() > MAX_FILE_BYTES {
        return Err(DiscordError::Invalid("data_base64"));
    }
    let body = embed::message(
        command_id,
        payload.get("text"),
        "attachments",
        json!([{
            "id":0,
            "filename":filename,
            "description":payload.get("description").and_then(Value::as_str)
        }]),
    )?;
    let content_type = payload
        .get("content_type")
        .and_then(Value::as_str)
        .unwrap_or("application/octet-stream")
        .to_owned();
    let url = effect::channel_url(config, channel, "messages");
    request::send(|| {
        let part = multipart::Part::bytes(data.clone())
            .file_name(filename.to_owned())
            .mime_str(&content_type)
            .map_err(|_error| DiscordError::Invalid("content_type"))?;
        let form = multipart::Form::new()
            .text("payload_json", body.to_string())
            .part("files[0]", part);
        Ok(request::auth(client.post(&url).multipart(form), config))
    })
}

fn string<'a>(payload: &'a Value, name: &'static str) -> Result<&'a str, DiscordError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or(DiscordError::Invalid(name))
}
