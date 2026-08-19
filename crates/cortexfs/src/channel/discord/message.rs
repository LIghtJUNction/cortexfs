use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::{DiscordConfig, DiscordError, effect};

pub(super) fn create(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    source: &str,
    text: &str,
) -> Result<String, DiscordError> {
    let value = effect::auth(
        client.post(effect::channel_url(config, channel, "messages")),
        config,
    )
    .json(&json!({
        "content": text,
        "message_reference": {"message_id": source},
        "allowed_mentions": {"parse": []}
    }))
    .send()
    .map_err(DiscordError::Http)?
    .error_for_status()
    .map_err(DiscordError::Http)?
    .json::<Value>()
    .map_err(DiscordError::Http)?;
    value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| DiscordError::Protocol("Discord progress message has no id".to_owned()))
}

pub(super) fn edit(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    message: &str,
    text: &str,
) -> Result<(), DiscordError> {
    effect::auth(
        client.patch(effect::channel_url(
            config,
            channel,
            &format!("messages/{message}"),
        )),
        config,
    )
    .json(&json!({"content": text, "allowed_mentions": {"parse": []}}))
    .send()
    .map_err(DiscordError::Http)?
    .error_for_status()
    .map_err(DiscordError::Http)?;
    Ok(())
}

pub(super) fn delete(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    message: &str,
) -> Result<(), DiscordError> {
    effect::auth(
        client.delete(effect::channel_url(
            config,
            channel,
            &format!("messages/{message}"),
        )),
        config,
    )
    .send()
    .map_err(DiscordError::Http)?
    .error_for_status()
    .map_err(DiscordError::Http)?;
    Ok(())
}
