use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::Value;

use super::super::{ChannelError, object};
use super::GmailPush;

pub(super) fn push_cursor(payload: &str) -> Result<Option<GmailPush>, ChannelError> {
    let root = object(payload)?;
    let Some(data) = root
        .get("message")
        .and_then(|message| message.get("data"))
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    let decoded = URL_SAFE_NO_PAD
        .decode(data.as_bytes())
        .map_err(|error| ChannelError::Protocol(format!("invalid Gmail push data: {error}")))?;
    let value: Value = serde_json::from_slice(&decoded)
        .map_err(|error| ChannelError::Protocol(format!("invalid Gmail push JSON: {error}")))?;
    Ok(Some(GmailPush {
        email_address: string(&value, "emailAddress")?,
        history_id: scalar(&value, "historyId")?,
    }))
}

pub(super) fn message(payload: &str) -> Result<Value, ChannelError> {
    object(payload)
}

pub(super) fn string(value: &Value, field: &str) -> Result<String, ChannelError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ChannelError::Protocol(format!("Gmail push field `{field}` is missing")))
}

pub(super) fn scalar(value: &Value, field: &str) -> Result<String, ChannelError> {
    string(value, field).or_else(|_| {
        value
            .get(field)
            .and_then(Value::as_u64)
            .map(|value| value.to_string())
            .ok_or_else(|| ChannelError::Protocol(format!("Gmail push field `{field}` is missing")))
    })
}

pub(super) fn header<'a>(root: &'a Value, name: &str) -> Option<&'a str> {
    root.get("payload")?
        .get("headers")?
        .as_array()?
        .iter()
        .find_map(|header| {
            let header_name = header.get("name").and_then(Value::as_str)?;
            header_name
                .eq_ignore_ascii_case(name)
                .then(|| header.get("value").and_then(Value::as_str))
                .flatten()
        })
}

pub(super) fn body_text(root: &Value) -> Option<String> {
    let body = root.get("payload")?;
    body_data(body).or_else(|| body.get("parts")?.as_array()?.iter().find_map(body_data))
}

fn body_data(value: &Value) -> Option<String> {
    let data = value.get("body")?.get("data")?.as_str()?;
    let bytes = URL_SAFE_NO_PAD.decode(data.as_bytes()).ok()?;
    String::from_utf8(bytes).ok()
}
