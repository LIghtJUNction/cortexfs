#![expect(
    clippy::field_scoped_visibility_modifiers,
    clippy::redundant_pub_crate,
    reason = "provider types are private driver plumbing"
)]

use std::collections::BTreeMap;

use cortexfs_channels::OutboundMessage;
use reqwest::Client;

use crate::{config::Config, error::Result};

pub(crate) mod call;
pub(crate) mod control;
mod incoming;
pub(crate) mod speech;

#[derive(Clone, Debug)]
pub(crate) struct ActiveCall {
    pub(crate) id: String,
}

pub(crate) type Calls = BTreeMap<String, ActiveCall>;

pub(crate) async fn send(
    config: &Config,
    client: &Client,
    calls: &mut Calls,
    message: &OutboundMessage,
) -> Result<String> {
    let conversation = message.target.conversation.as_str();
    let active = message
        .metadata
        .get("voice_call_id")
        .map(|id| ActiveCall { id: id.clone() })
        .or_else(|| calls.get(conversation).cloned());
    if let Some(active) = active {
        return speech::apply(config, client, calls, &active, message).await;
    }
    let destination = destination(message);
    if !config.accepts(destination) {
        return Err(crate::error::Error::Protocol(
            "voice destination is not allowlisted".to_owned(),
        ));
    }
    let id = call::place(config, client, destination).await?;
    let active = ActiveCall { id: id.clone() };
    calls.insert(format!("call:{id}"), active.clone());
    calls.insert(format!("phone:{destination}"), active.clone());
    if config.channel == crate::config::ChannelKind::ClawdTalk {
        speech::speak(config, client, &active, &message.body.text).await?;
    }
    Ok(id)
}

pub(crate) fn incoming(
    config: &Config,
    content_type: &str,
    body: &str,
    calls: &mut Calls,
) -> Result<Option<cortexfs_channels::InboundMessage>> {
    incoming::decode(config, content_type, body, calls)
}

fn destination(message: &OutboundMessage) -> &str {
    message
        .metadata
        .get("voice_destination")
        .map(String::as_str)
        .or_else(|| message.target.conversation.as_str().strip_prefix("phone:"))
        .unwrap_or_else(|| {
            message
                .target
                .conversation
                .as_str()
                .strip_prefix("call:")
                .unwrap_or(message.target.conversation.as_str())
        })
}
