use std::{thread, time::Duration};

use cortexfs_channels::{ChannelCodec, ChannelError, platform::reddit::RedditCodec};
use reqwest::blocking::Client;

use super::bridge::{AgentChannelBridge, ChannelBridgeError};

mod api;
mod config;
mod control;

#[cfg(test)]
mod tests;

pub use config::RedditConfig;

/// Runs a reconnecting Reddit OAuth inbox poller.
pub fn run(config: &RedditConfig, bridge: &AgentChannelBridge) -> Result<(), RedditError> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(RedditError::Http)?;
    let mut session = api::login(&client, config)?;
    let control = control::start(config, bridge, &client)?;
    loop {
        control
            .check()
            .map_err(|error| RedditError::Protocol(error.to_string()))?;
        match poll(&client, config, bridge, &mut session) {
            Ok(()) => thread::sleep(config.poll_delay()),
            Err(_error) => {
                thread::sleep(Duration::from_secs(5));
                session = api::login(&client, config)?;
            }
        }
    }
}

fn poll(
    client: &Client,
    config: &RedditConfig,
    bridge: &AgentChannelBridge,
    session: &mut api::Session,
) -> Result<(), RedditError> {
    let codec = RedditCodec;
    let messages = codec.decode_many(&api::inbox(client, config, session)?)?;
    let mut read_ids = Vec::new();
    for inbound in messages {
        if let Some(name) = inbound.metadata.get("reddit.name") {
            read_ids.push(name.clone());
        }
        if inbound.sender.id.eq_ignore_ascii_case(&config.username)
            || !config.accepts(inbound.metadata.get("reddit.subreddit"))
        {
            continue;
        }
        let outbound = bridge.handle(inbound)?;
        api::send(client, config, session, codec.encode(&outbound)?)?;
    }
    api::mark_read(client, config, session, &read_ids)
}

#[derive(Debug, thiserror::Error)]
pub enum RedditError {
    #[error("Reddit configuration failed: {0}")]
    Config(String),
    #[error("Reddit HTTP request failed")]
    Http(#[source] reqwest::Error),
    #[error(transparent)]
    Channel(#[from] ChannelError),
    #[error(transparent)]
    Bridge(#[from] ChannelBridgeError),
    #[error("Reddit response is invalid: {0}")]
    Protocol(String),
}
