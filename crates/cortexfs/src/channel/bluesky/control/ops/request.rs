use cortexfs_channels::OutboundRequest;
use reqwest::blocking::Client;
use serde_json::Value;

use super::super::super::{BlueskyConfig, api};
use crate::channel::control::ChannelControlError;

pub(super) fn record(
    client: &Client,
    config: &BlueskyConfig,
    session: &mut api::Session,
    collection: &str,
    value: &Value,
) -> Result<Value, ChannelControlError> {
    api::send(
        client,
        config,
        session,
        request(
            "com.atproto.repo.createRecord",
            &serde_json::json!({
                "repo": session.did,
                "collection": collection,
                "record": value,
            }),
        ),
    )
    .map_err(|error| fail(&error.to_string()))?;
    Ok(serde_json::json!({"accepted":true}))
}

pub(super) fn remove(
    client: &Client,
    config: &BlueskyConfig,
    session: &mut api::Session,
    collection: &str,
    rkey: &str,
) -> Result<Value, ChannelControlError> {
    api::send(
        client,
        config,
        session,
        request(
            "com.atproto.repo.deleteRecord",
            &serde_json::json!({
                "repo": session.did,
                "collection": collection,
                "rkey": rkey,
            }),
        ),
    )
    .map_err(|error| fail(&error.to_string()))?;
    Ok(serde_json::json!({"accepted":true}))
}

pub(super) fn field<'a>(
    payload: &'a Value,
    name: &'static str,
) -> Result<&'a str, ChannelControlError> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && !value.contains('\0'))
        .ok_or_else(|| fail(&format!("{name} is missing")))
}

fn request(path: &str, body: &Value) -> OutboundRequest {
    OutboundRequest {
        method: "POST".to_owned(),
        path: path.to_owned(),
        content_type: "application/json".to_owned(),
        body: body.to_string(),
        headers: std::collections::BTreeMap::new(),
    }
}

fn fail(message: &str) -> ChannelControlError {
    ChannelControlError::Operation(message.to_owned())
}
