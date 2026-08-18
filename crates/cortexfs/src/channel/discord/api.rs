use reqwest::blocking::Client;
use serde_json::json;

use super::{DiscordConfig, DiscordError};

const MAX_REPLY_BYTES: usize = 64 * 1024;
const DISCORD_MESSAGE_CHARS: usize = 1_900;

pub(super) fn verify_application(
    client: &Client,
    config: &DiscordConfig,
) -> Result<(), DiscordError> {
    let url = format!("{}/applications/@me", config.api_base.trim_end_matches('/'));
    let response = client
        .get(url)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bot {}", config.bot_token),
        )
        .send()
        .map_err(DiscordError::Http)?
        .error_for_status()
        .map_err(DiscordError::Http)?;
    let body = response.text().map_err(DiscordError::Http)?;
    let value: serde_json::Value = serde_json::from_str(&body)?;
    let actual = value.get("id").and_then(serde_json::Value::as_str);
    if actual != Some(config.application_id.as_str()) {
        return Err(DiscordError::Protocol(
            "application_id does not match bot token".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn send_reply(
    client: &Client,
    config: &DiscordConfig,
    channel_id: &str,
    text: &str,
) -> Result<(), DiscordError> {
    let mut chunk = String::new();
    let mut count = 0_usize;
    let mut bytes = 0_usize;
    for character in text.chars() {
        let width = character.len_utf8();
        if bytes.saturating_add(width) > MAX_REPLY_BYTES {
            break;
        }
        chunk.push(character);
        bytes += width;
        count += 1;
        if count == DISCORD_MESSAGE_CHARS {
            send_chunk(client, config, channel_id, &chunk)?;
            chunk.clear();
            count = 0;
        }
    }
    if !chunk.is_empty() {
        send_chunk(client, config, channel_id, &chunk)?;
    }
    Ok(())
}

fn send_chunk(
    client: &Client,
    config: &DiscordConfig,
    channel_id: &str,
    text: &str,
) -> Result<(), DiscordError> {
    let url = format!(
        "{}/channels/{channel_id}/messages",
        config.api_base.trim_end_matches('/')
    );
    client
        .post(url)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bot {}", config.bot_token),
        )
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(json!({ "content": text, "allowed_mentions": { "parse": [] } }).to_string())
        .send()
        .map_err(DiscordError::Http)?
        .error_for_status()
        .map_err(DiscordError::Http)?;
    Ok(())
}
