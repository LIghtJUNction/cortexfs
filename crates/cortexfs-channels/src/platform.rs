use std::collections::BTreeMap;

use serde_json::Value;

use crate::{ChannelError, ChannelId, InboundMessage, OutboundMessage, Participant};

pub mod discord;
pub mod feishu;
pub mod slack;
pub mod telegram;

/// Platform-neutral HTTP operation emitted by a webhook codec.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboundRequest {
    pub method: String,
    pub path: String,
    pub content_type: String,
    pub body: String,
    pub headers: BTreeMap<String, String>,
}

/// Stateless codec for one platform's webhook payload and send shape.
pub trait ChannelCodec: Send + Sync {
    fn channel(&self) -> ChannelId;
    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError>;
    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError>;
    fn challenge(&self, _payload: &str) -> Option<String> {
        None
    }
}

pub(crate) fn object(payload: &str) -> Result<Value, ChannelError> {
    let value: Value = serde_json::from_str(payload)
        .map_err(|error| ChannelError::Protocol(format!("invalid webhook JSON: {error}")))?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(ChannelError::Protocol(
            "webhook payload is not an object".to_owned(),
        ))
    }
}

pub(crate) fn string(value: Option<&Value>, field: &str) -> Result<String, ChannelError> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| ChannelError::Protocol(format!("webhook field `{field}` is missing")))
}

pub(crate) fn scalar(value: Option<&Value>, field: &str) -> Result<String, ChannelError> {
    if let Some(value) = value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    {
        return Ok(value.to_owned());
    }
    value
        .and_then(Value::as_number)
        .map(ToString::to_string)
        .ok_or_else(|| ChannelError::Protocol(format!("webhook field `{field}` is missing")))
}

pub(crate) fn participant(value: Option<&Value>, id: String) -> Participant {
    Participant {
        id,
        display_name: value
            .and_then(|item| item.get("display_name").or_else(|| item.get("username")))
            .and_then(Value::as_str)
            .map(str::to_owned),
        handle: value
            .and_then(|item| item.get("username").or_else(|| item.get("handle")))
            .and_then(Value::as_str)
            .map(str::to_owned),
    }
}

pub(crate) fn timestamp_ms(value: Option<&Value>) -> Option<u64> {
    value
        .and_then(|item| item.as_u64().or_else(|| item.as_str()?.parse().ok()))
        .map(|seconds| seconds.saturating_mul(1_000))
}

pub(crate) fn text(value: Option<&Value>) -> Result<crate::MessageBody, ChannelError> {
    crate::MessageBody::text(string(value, "text")?)
}
