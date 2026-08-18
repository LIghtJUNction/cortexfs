use reqwest::blocking::Client;
use serde_json::{Map, Value, json};

use super::{TelegramConfig, TelegramError, api};

pub(super) fn call(
    client: &Client,
    config: &TelegramConfig,
    method: &str,
    fields: Map<String, Value>,
) -> Result<Value, TelegramError> {
    let value = client
        .post(api::api_url(config, method))
        .json(&Value::Object(fields))
        .send()
        .map_err(TelegramError::Http)?
        .error_for_status()
        .map_err(TelegramError::Http)?
        .json::<Value>()
        .map_err(TelegramError::Http)?;
    if value.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(TelegramError::Api(
            value
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("Telegram request failed")
                .to_owned(),
        ));
    }
    Ok(value)
}

pub(super) fn delete(
    client: &Client,
    config: &TelegramConfig,
    chat: &str,
    message: &str,
) -> Result<(), TelegramError> {
    let mut fields = Map::new();
    fields.insert("chat_id".to_owned(), json!(chat));
    fields.insert("message_id".to_owned(), json!(message));
    let _ignored = call(client, config, "deleteMessage", fields)?;
    Ok(())
}
