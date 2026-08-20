use cortexfs::channel::control::ChannelControlError;
use cortexfs_channels::OutboundRequest;
use reqwest::blocking::Client;
use serde_json::Value;

use super::super::{TwitterConfig, api};

pub(super) fn like(
    client: &Client,
    config: &TwitterConfig,
    bot_id: &str,
    payload: &Value,
) -> Result<Value, ChannelControlError> {
    let request = request(
        format!("users/{bot_id}/likes"),
        &serde_json::json!({"tweet_id": string(payload, "tweet_id")?}),
    );
    api::send(client, config, request).map_err(|error| fail(&error.to_string()))?;
    Ok(serde_json::json!({"accepted":true}))
}

pub(super) fn search(
    client: &Client,
    config: &TwitterConfig,
    bot_id: &str,
) -> Result<Value, ChannelControlError> {
    serde_json::from_str(
        &api::mentions(client, config, bot_id, None).map_err(|error| fail(&error.to_string()))?,
    )
    .map_err(|error| fail(&error.to_string()))
}

pub(super) fn dm(
    client: &Client,
    config: &TwitterConfig,
    payload: &Value,
) -> Result<Value, ChannelControlError> {
    let request = request(
        format!(
            "dm_conversations/with/{}/messages",
            string(payload, "participant_id")?
        ),
        &serde_json::json!({"text": string(payload, "text")?}),
    );
    api::send(client, config, request).map_err(|error| fail(&error.to_string()))?;
    Ok(serde_json::json!({"accepted":true}))
}

fn string<'a>(payload: &'a Value, name: &'static str) -> Result<&'a str, ChannelControlError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or_else(|| fail(&format!("{name} is missing")))
}

fn request(path: String, body: &Value) -> OutboundRequest {
    OutboundRequest {
        method: "POST".to_owned(),
        path,
        content_type: "application/json".to_owned(),
        body: body.to_string(),
        headers: std::collections::BTreeMap::new(),
    }
}

fn fail(message: &str) -> ChannelControlError {
    ChannelControlError::Operation(message.to_owned())
}
