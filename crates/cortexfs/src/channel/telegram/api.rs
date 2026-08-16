use cortexfs_channels::OutboundRequest;
use serde_json::{Value, json};

use super::{TelegramConfig, TelegramError};

pub(super) fn get_updates(
    client: &reqwest::blocking::Client,
    config: &TelegramConfig,
    offset: i64,
) -> Result<Vec<Value>, TelegramError> {
    let response = client
        .post(api_url(config, "getUpdates"))
        .json(&json!({ "offset": offset, "timeout": config.poll_seconds }))
        .send()
        .map_err(TelegramError::Http)?
        .error_for_status()
        .map_err(TelegramError::Http)?
        .json::<Value>()
        .map_err(TelegramError::Http)?;
    api_result(&response)
}

pub(super) fn send_message(
    client: &reqwest::blocking::Client,
    config: &TelegramConfig,
    request: OutboundRequest,
) -> Result<(), TelegramError> {
    client
        .post(api_url(config, &request.path))
        .header(reqwest::header::CONTENT_TYPE, request.content_type)
        .body(request.body)
        .send()
        .map_err(TelegramError::Http)?
        .error_for_status()
        .map_err(TelegramError::Http)?;
    Ok(())
}

fn api_result(value: &Value) -> Result<Vec<Value>, TelegramError> {
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(TelegramError::Api(
            value
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("unknown Telegram error")
                .to_owned(),
        ));
    }
    value
        .get("result")
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| TelegramError::Api("Telegram result is not an array".to_owned()))
}

fn api_url(config: &TelegramConfig, method: &str) -> String {
    format!(
        "{}/bot{}/{}",
        config.api_base.trim_end_matches('/'),
        config.token,
        method
    )
}
