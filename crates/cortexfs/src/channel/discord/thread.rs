use reqwest::blocking::Client;
use serde_json::{Map, Value, json};

use super::{DiscordConfig, DiscordError, effect, request, required_string};

pub(super) fn create(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    payload: &Value,
) -> Result<Value, DiscordError> {
    let message = required_string(payload, "message_id")?;
    if !message.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(DiscordError::Invalid("message_id"));
    }
    let name = required_string(payload, "name")?;
    if !(1..=100).contains(&name.chars().count()) {
        return Err(DiscordError::Invalid("name"));
    }
    let mut body = Map::new();
    body.insert("name".to_owned(), json!(name));
    if let Some(value) = payload.get("auto_archive_duration").and_then(Value::as_u64) {
        if ![60, 1_440, 4_320, 10_080].contains(&value) {
            return Err(DiscordError::Invalid("auto_archive_duration"));
        }
        body.insert("auto_archive_duration".to_owned(), json!(value));
    }
    if let Some(value) = payload.get("rate_limit_per_user").and_then(Value::as_u64) {
        if value > 21_600 {
            return Err(DiscordError::Invalid("rate_limit_per_user"));
        }
        body.insert("rate_limit_per_user".to_owned(), json!(value));
    }
    let url = effect::channel_url(config, channel, &format!("messages/{message}/threads"));
    match request::send(|| Ok(request::auth(client.post(&url).json(&body), config))) {
        Ok(value) => Ok(value),
        Err(error) => existing(client, config, channel, message).ok_or(error),
    }
}

fn existing(client: &Client, config: &DiscordConfig, parent: &str, thread: &str) -> Option<Value> {
    let url = format!(
        "{}/channels/{thread}",
        config.api_base.trim_end_matches('/')
    );
    request::send(|| Ok(request::auth(client.get(&url), config)))
        .ok()
        .filter(|value| {
            value.get("id").and_then(Value::as_str) == Some(thread)
                && value.get("parent_id").and_then(Value::as_str) == Some(parent)
                && value
                    .get("type")
                    .and_then(Value::as_u64)
                    .is_some_and(|kind| matches!(kind, 10..=12))
        })
}
