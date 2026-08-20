use std::collections::BTreeMap;

use serde_json::Value;

use crate::{
    ChannelEffect, ChannelError, ChannelId, ChannelIncoming, ChannelIncomingEvent, InboundMessage,
    MessageTarget, OutboundMessage, Participant,
};

pub mod bluesky;
pub mod catalog;
pub mod dingtalk;
pub mod discord;
pub mod email;
pub mod feishu;
pub mod gmail;
pub mod irc;
pub mod lark;
pub mod line;
pub mod linq;
pub mod matrix;
pub mod mattermost;
pub mod mochat;
pub mod nextcloud;
pub mod notion;
pub mod qq;
pub mod reddit;
pub mod signal;
pub mod slack;
pub mod teams;
pub mod telegram;
pub mod twitch;
pub mod twitter;
pub mod wecom;
pub mod whatsapp;

mod tool;

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
    fn capabilities(&self) -> crate::ChannelCapabilities {
        catalog::find(self.channel().as_str())
            .map_or_else(crate::ChannelCapabilities::text, |spec| spec.capabilities)
    }
    fn actions(&self) -> crate::ChannelActions {
        catalog::find(self.channel().as_str())
            .map_or_else(crate::ChannelActions::empty, |spec| spec.actions())
    }
    fn decode(&self, payload: &str) -> Result<Option<InboundMessage>, ChannelError>;
    fn decode_event(&self, _payload: &str) -> Result<Option<ChannelIncomingEvent>, ChannelError> {
        Ok(None)
    }
    fn decode_incoming(&self, payload: &str) -> Result<Option<ChannelIncoming>, ChannelError> {
        if let Some(event) = self.decode_event(payload)? {
            return Ok(Some(ChannelIncoming::Event(event)));
        }
        Ok(self.decode(payload)?.map(ChannelIncoming::Message))
    }
    fn decode_incoming_for(
        &self,
        channel: ChannelId,
        payload: &str,
    ) -> Result<Option<ChannelIncoming>, ChannelError> {
        self.decode_incoming(payload)
            .map(|incoming| incoming.map(|item| item.with_channel(channel)))
    }
    fn decode_many(&self, payload: &str) -> Result<Vec<InboundMessage>, ChannelError> {
        Ok(self.decode(payload)?.into_iter().collect())
    }
    fn decode_many_incoming(&self, payload: &str) -> Result<Vec<ChannelIncoming>, ChannelError> {
        if let Some(event) = self.decode_event(payload)? {
            return Ok(vec![ChannelIncoming::Event(event)]);
        }
        Ok(self
            .decode_many(payload)?
            .into_iter()
            .map(ChannelIncoming::Message)
            .collect())
    }
    fn decode_many_incoming_for(
        &self,
        channel: ChannelId,
        payload: &str,
    ) -> Result<Vec<ChannelIncoming>, ChannelError> {
        self.decode_many_incoming(payload).map(|items| {
            items
                .into_iter()
                .map(|item| item.with_channel(channel.clone()))
                .collect()
        })
    }
    fn encode(&self, message: &OutboundMessage) -> Result<OutboundRequest, ChannelError>;
    fn encode_effect(
        &self,
        _target: &MessageTarget,
        effect: &ChannelEffect,
    ) -> Result<Option<OutboundRequest>, ChannelError> {
        effect.validate()?;
        Ok(None)
    }
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
