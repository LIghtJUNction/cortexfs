use cortexfs_channels::MessageTarget;
use reqwest::{Method, blocking::Client};
use serde_json::{Value, json};

use super::super::{NotionConfig, NotionError, api};
use crate::channel::control::ChannelControlError;

pub(super) fn run(
    client: &Client,
    config: &NotionConfig,
    target: Option<&MessageTarget>,
    name: &str,
    payload: &Value,
) -> Result<Value, ChannelControlError> {
    let value = match name {
        "notion.query_database" => api::request(
            client,
            config,
            Method::POST,
            &format!("databases/{}/query", config.database_id),
            Some(payload.clone()),
        ),
        "notion.create_page" => api::request(
            client,
            config,
            Method::POST,
            "pages",
            Some(payload.clone()),
        ),
        "notion.update_page" => api::request(
            client,
            config,
            Method::PATCH,
            &format!("pages/{}", page(target, payload)?),
            Some(payload.clone()),
        ),
        "notion.append_block" => api::request(
            client,
            config,
            Method::PATCH,
            &format!("blocks/{}/children", page(target, payload)?),
            Some(json!({"children": payload.get("children").cloned().unwrap_or(Value::Array(Vec::new()))})),
        ),
        _ => Err(NotionError::Protocol("unsupported operation".to_owned())),
    }
    .map_err(|error| fail(&error.to_string()))?;
    Ok(value)
}

fn page(target: Option<&MessageTarget>, payload: &Value) -> Result<String, ChannelControlError> {
    payload
        .get("page_id")
        .and_then(Value::as_str)
        .or_else(|| target.map(|item| item.conversation.as_str()))
        .filter(|value| !value.is_empty() && !value.contains(['/', '?', '#', '\0']))
        .map(str::to_owned)
        .ok_or_else(|| fail("page_id is missing"))
}

fn fail(message: &str) -> ChannelControlError {
    ChannelControlError::Operation(message.to_owned())
}
