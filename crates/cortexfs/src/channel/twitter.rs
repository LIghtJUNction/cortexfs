use std::{thread, time::Duration};

use cortexfs_channels::{ChannelCodec, platform::twitter::TwitterCodec};
use reqwest::blocking::Client;
use serde_json::Value;

use super::bridge::{AgentChannelBridge, ChannelBridgeError};

mod api;
mod config;
mod text;

#[cfg(test)]
mod tests;

pub use config::TwitterConfig;

/// Runs a reconnecting Twitter API v2 mentions poller.
pub fn run(config: &TwitterConfig, bridge: &AgentChannelBridge) -> Result<(), TwitterError> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(TwitterError::Http)?;
    let bot_id = api::me(&client, config)?;
    let mut since_id = None;
    loop {
        match poll(&client, config, bridge, &bot_id, &mut since_id) {
            Ok(()) => thread::sleep(config.poll_delay()),
            Err(TwitterError::RateLimited) => thread::sleep(Duration::from_mins(1)),
            Err(_error) => thread::sleep(Duration::from_secs(5)),
        }
    }
}

fn poll(
    client: &Client,
    config: &TwitterConfig,
    bridge: &AgentChannelBridge,
    bot_id: &str,
    since_id: &mut Option<String>,
) -> Result<(), TwitterError> {
    let codec = TwitterCodec;
    let payload = api::mentions(client, config, bot_id, since_id.as_deref())?;
    let root: Value = serde_json::from_str(&payload)
        .map_err(|error| TwitterError::Protocol(error.to_string()))?;
    let messages = codec.decode_many(&payload)?;
    for inbound in messages.into_iter().rev() {
        advance(
            since_id,
            inbound.metadata.get("twitter.tweet_id").map(String::as_str),
        );
        if inbound.sender.id == bot_id
            || !config.accepts(
                &inbound.sender.id,
                inbound.metadata.get("twitter.username").map(String::as_str),
            )
        {
            continue;
        }
        deliver(client, config, bridge, codec, inbound)?;
    }
    advance(
        since_id,
        root.get("meta")
            .and_then(|meta| meta.get("newest_id"))
            .and_then(Value::as_str),
    );
    Ok(())
}

fn deliver(
    client: &Client,
    config: &TwitterConfig,
    bridge: &AgentChannelBridge,
    codec: TwitterCodec,
    inbound: cortexfs_channels::InboundMessage,
) -> Result<(), TwitterError> {
    let outbound = bridge.handle(inbound)?;
    let mut reply_to = outbound.target.reply_to.clone();
    for chunk in text::chunks(&outbound.body.text, 280) {
        let mut message = outbound.clone();
        message.body.text = chunk;
        message.target.reply_to.clone_from(&reply_to);
        reply_to = api::send(client, config, codec.encode(&message)?)?;
    }
    Ok(())
}

fn advance(cursor: &mut Option<String>, value: Option<&str>) {
    if let Some(value) = value.filter(|value| !value.is_empty())
        && cursor.as_deref().is_none_or(|current| value > current)
    {
        *cursor = Some(value.to_owned());
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TwitterError {
    #[error("Twitter configuration failed: {0}")]
    Config(String),
    #[error("Twitter HTTP request failed")]
    Http(#[source] reqwest::Error),
    #[error(transparent)]
    Channel(#[from] cortexfs_channels::ChannelError),
    #[error(transparent)]
    Bridge(#[from] ChannelBridgeError),
    #[error("Twitter API rate limit reached")]
    RateLimited,
    #[error("Twitter response is invalid: {0}")]
    Protocol(String),
}
