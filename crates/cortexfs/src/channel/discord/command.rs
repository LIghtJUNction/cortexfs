use reqwest::blocking::Client;
use serde_json::{Value, json};

use super::{DiscordConfig, DiscordError, embed, message, request};

pub(super) fn register(
    client: &Client,
    config: &DiscordConfig,
    payload: &Value,
) -> Result<Value, DiscordError> {
    let command = payload.get("command").unwrap_or(payload);
    let object = command
        .as_object()
        .ok_or(DiscordError::Invalid("command"))?;
    let name = object
        .get("name")
        .and_then(Value::as_str)
        .filter(|value| (1..=32).contains(&value.chars().count()))
        .ok_or(DiscordError::Invalid("command.name"))?;
    if name.contains(' ') {
        return Err(DiscordError::Invalid("command.name"));
    }
    let url = format!(
        "{}/applications/{}/commands",
        config.api_base.trim_end_matches('/'),
        config.application_id
    );
    request::send(|| Ok(request::auth(client.post(&url).json(command), config)))
}
pub(super) fn autocomplete(
    client: &Client,
    config: &DiscordConfig,
    payload: &Value,
) -> Result<Value, DiscordError> {
    let interaction = string(payload, "interaction_id")?;
    let token = string(payload, "interaction_token")?;
    let choices = payload
        .get("choices")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty() && items.len() <= 25)
        .ok_or(DiscordError::Invalid("choices"))?;
    let url = format!(
        "{}/interactions/{interaction}/{token}/callback",
        config.api_base.trim_end_matches('/')
    );
    request::send(|| {
        Ok(request::auth(
            client
                .post(&url)
                .json(&json!({"type": 8, "data": {"choices": choices}})),
            config,
        ))
    })
}
pub(super) fn gate_prompt(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    command_id: &str,
    payload: &Value,
) -> Result<Value, DiscordError> {
    let title = string(payload, "title")?;
    let description = string(payload, "description")?;
    let choices = payload
        .get("choices")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty() && items.len() <= 5)
        .ok_or(DiscordError::Invalid("choices"))?;
    let components = choices.iter().map(button).collect::<Result<Vec<_>, _>>()?;
    let body = embed::message(
        command_id,
        None,
        "embeds",
        json!([{"title": title, "description": description}]),
    )?;
    let mut body = body;
    body.as_object_mut()
        .ok_or(DiscordError::Invalid("gate"))?
        .insert(
            "components".to_owned(),
            json!([{"type": 1, "components": components}]),
        );
    embed::post(client, config, channel, &body)
}
pub(super) fn gate_finalize(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    payload: &Value,
) -> Result<Value, DiscordError> {
    let message_id = string(payload, "message_id")?;
    let outcome = string(payload, "outcome")?;
    message::edit_components(client, config, channel, message_id, outcome)
}
pub(super) fn draft_update(
    client: &Client,
    config: &DiscordConfig,
    channel: &str,
    payload: &Value,
) -> Result<Value, DiscordError> {
    let message_id = string(payload, "message_id")?;
    let text = string(payload, "text")?;
    message::edit(client, config, channel, message_id, text)?;
    Ok(json!({"message_id": message_id, "updated": true}))
}
fn button(value: &Value) -> Result<Value, DiscordError> {
    let id = string(value, "id")?;
    let label = string(value, "label")?;
    if id.len() > 100 || label.chars().count() > 80 {
        return Err(DiscordError::Invalid("choice"));
    }
    Ok(json!({"type": 2, "style": 1, "custom_id": id, "label": label}))
}
fn string<'a>(value: &'a Value, name: &'static str) -> Result<&'a str, DiscordError> {
    value
        .get(name)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty() && !text.contains('\0'))
        .ok_or(DiscordError::Invalid(name))
}
