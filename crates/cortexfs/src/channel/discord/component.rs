use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::{DiscordConfig, DiscordError, embed};

pub(super) fn send(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    command_id: &str,
    payload: &Value,
) -> Result<Value, DiscordError> {
    let components = payload
        .get("components")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty() && items.len() <= 40)
        .ok_or(DiscordError::Invalid("components"))?;
    if serde_json::to_vec(components)?.len() > 128 * 1024 {
        return Err(DiscordError::Invalid("components"));
    }
    let mut body = embed::message(
        command_id,
        payload.get("text"),
        "components",
        json!(components),
    )?;
    if let Some(flags) = payload.get("flags").and_then(Value::as_u64) {
        if flags & 32_768 != 0 && payload.get("text").is_some() {
            return Err(DiscordError::Invalid("components v2 content"));
        }
        body.as_object_mut()
            .ok_or(DiscordError::Invalid("components"))?
            .insert("flags".to_owned(), json!(flags));
    }
    embed::post(client, config, channel, &body)
}
