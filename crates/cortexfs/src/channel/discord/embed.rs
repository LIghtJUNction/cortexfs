use std::fmt::Write as _;

use reqwest::blocking::Client;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use super::{DiscordConfig, DiscordError, effect, request};

const EMBED_FIELDS: &[&str] = &[
    "title",
    "description",
    "url",
    "timestamp",
    "color",
    "footer",
    "image",
    "thumbnail",
    "author",
    "fields",
];

pub(super) fn send(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    command_id: &str,
    payload: &Value,
) -> Result<Value, DiscordError> {
    let source = payload.get("embed").unwrap_or(payload);
    let source = source.as_object().ok_or(DiscordError::Invalid("embed"))?;
    let mut embed = Map::new();
    for &name in EMBED_FIELDS {
        if let Some(value) = source.get(name) {
            embed.insert(name.to_owned(), value.clone());
        }
    }
    if embed.is_empty() || serde_json::to_vec(&embed)?.len() > 64 * 1024 {
        return Err(DiscordError::Invalid("embed"));
    }
    let body = message(command_id, payload.get("text"), "embeds", json!([embed]))?;
    post(client, config, channel, &body)
}

pub(super) fn message(
    command_id: &str,
    text: Option<&Value>,
    field: &str,
    value: Value,
) -> Result<Value, DiscordError> {
    let mut body = Map::new();
    if let Some(text) = text {
        let text = text.as_str().ok_or(DiscordError::Invalid("text"))?;
        if text.chars().count() > 2_000 {
            return Err(DiscordError::Invalid("text"));
        }
        body.insert("content".to_owned(), json!(text));
    }
    body.insert(field.to_owned(), value);
    body.insert("allowed_mentions".to_owned(), json!({"parse":[]}));
    body.insert("nonce".to_owned(), json!(nonce(command_id)));
    body.insert("enforce_nonce".to_owned(), json!(true));
    Ok(Value::Object(body))
}

pub(super) fn post(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    body: &Value,
) -> Result<Value, DiscordError> {
    let url = effect::channel_url(config, channel, "messages");
    request::send(|| Ok(request::auth(client.post(&url).json(body), config)))
}

fn nonce(command_id: &str) -> String {
    let digest = Sha256::digest(command_id.as_bytes());
    let mut nonce = String::with_capacity(24);
    for byte in digest.iter().take(12) {
        let _ignored = write!(nonce, "{byte:02x}");
    }
    nonce
}
