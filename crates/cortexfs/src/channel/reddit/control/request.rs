use cortexfs_channels::{MessageTarget, OutboundRequest};
use reqwest::blocking::Client;
use serde_json::Value;

use super::super::{RedditConfig, api};
use crate::channel::control::ChannelControlError;

pub(super) fn post<const N: usize>(
    client: &Client,
    config: &RedditConfig,
    session: &mut api::Session,
    path: &str,
    fields: [(&str, String); N],
) -> Result<bool, ChannelControlError> {
    let mut form = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in fields {
        form.append_pair(key, &value);
    }
    api::send(
        client,
        config,
        session,
        OutboundRequest {
            method: "POST".to_owned(),
            path: path.to_owned(),
            content_type: "application/x-www-form-urlencoded".to_owned(),
            body: form.finish(),
            headers: std::collections::BTreeMap::new(),
        },
    )
    .map_err(|error| fail(&error.to_string()))?;
    Ok(true)
}

pub(super) fn target_id(
    target: Option<&MessageTarget>,
    payload: &Value,
    name: &'static str,
) -> Result<String, ChannelControlError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .or_else(|| target.and_then(|item| item.reply_to.as_deref()))
        .or_else(|| target.map(|item| item.conversation.as_str()))
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .map(str::to_owned)
        .ok_or_else(|| fail(&format!("{name} is missing")))
}

pub(super) fn value(payload: &Value, name: &'static str) -> Result<String, ChannelControlError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .map(str::to_owned)
        .ok_or_else(|| fail(&format!("{name} is missing")))
}

fn fail(message: &str) -> ChannelControlError {
    ChannelControlError::Operation(message.to_owned())
}
