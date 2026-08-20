use std::{thread, time::Duration};

use cortexfs_channels::{ChannelCodec, platform::bluesky::BlueskyCodec};
use reqwest::blocking::Client;

use super::bridge::AgentChannelBridge;

mod api;
mod clock;
mod config;
mod control;

#[cfg(test)]
mod tests;

pub use config::{BlueskyConfig, BlueskyError};

/// Runs a reconnecting AT Protocol notification poller.
pub fn run(config: &BlueskyConfig, bridge: &AgentChannelBridge) -> Result<(), BlueskyError> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(BlueskyError::Http)?;
    let mut session = api::login(&client, config)?;
    let control = control::start(config, bridge, &client)?;
    loop {
        control
            .check()
            .map_err(|error| BlueskyError::Protocol(error.to_string()))?;
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
    config: &BlueskyConfig,
    bridge: &AgentChannelBridge,
    session: &mut api::Session,
) -> Result<(), BlueskyError> {
    let codec = BlueskyCodec;
    let payload = api::notifications(client, config, session)?;
    let messages = codec.decode_many(&payload)?;
    let mut seen = None;
    for inbound in messages {
        if inbound.sender.id == session.did {
            continue;
        }
        seen = inbound.metadata.get("bluesky.seen_at").cloned().or(seen);
        let mut outbound = bridge.handle(inbound)?;
        outbound
            .metadata
            .insert("bluesky.repo".to_owned(), session.did.clone());
        outbound
            .metadata
            .insert("bluesky.created_at".to_owned(), clock::now());
        let request = codec.encode(&outbound)?;
        api::send(client, config, session, request)?;
    }
    if let Some(seen) = seen {
        api::mark_seen(client, config, session, &seen)?;
    }
    Ok(())
}
