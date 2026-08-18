use std::{collections::HashSet, thread, time::Duration};

use cortexfs_channels::{ChannelCodec, platform::notion::NotionCodec};
use reqwest::blocking::Client;
use serde_json::Value;

use super::bridge::{AgentChannelBridge, ChannelBridgeError};

mod api;
mod config;

#[cfg(test)]
mod tests;

pub use config::NotionConfig;

/// Runs the Notion database task poller.
pub fn run(config: &NotionConfig, bridge: &AgentChannelBridge) -> Result<(), NotionError> {
    let client = Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(NotionError::Http)?;
    let status_type = if config.status_type == "auto" {
        api::status_type(&client, config)?
    } else {
        config.status_type.clone()
    };
    if config.recover_stale {
        api::recover_stale(&client, config, &status_type)?;
    }
    let codec = NotionCodec::new(
        &config.status_property,
        &config.input_property,
        &config.result_property,
    )
    .with_status_type(&status_type);
    let mut active = HashSet::new();
    loop {
        if let Err(_error) = poll(&client, config, &codec, bridge, &mut active) {
            thread::sleep(Duration::from_secs(5));
        } else {
            thread::sleep(config.poll_delay());
        }
    }
}

fn poll(
    client: &Client,
    config: &NotionConfig,
    codec: &NotionCodec,
    bridge: &AgentChannelBridge,
    active: &mut HashSet<String>,
) -> Result<(), NotionError> {
    let pages = api::pending(client, config, codec.status_type())?;
    for page in pages.into_iter().take(config.max_concurrent) {
        let Some(id) = page.get("id").and_then(Value::as_str) else {
            continue;
        };
        if !active.insert(id.to_owned()) {
            continue;
        }
        let result = process_page(client, config, codec, bridge, &page);
        active.remove(id);
        if let Err(error) = result {
            api::mark_failed(client, config, codec, id, error.safe_message())?;
        }
    }
    Ok(())
}

fn process_page(
    client: &Client,
    config: &NotionConfig,
    codec: &NotionCodec,
    bridge: &AgentChannelBridge,
    page: &Value,
) -> Result<(), NotionError> {
    let id = page
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| NotionError::Protocol("Notion page id is missing".to_owned()))?;
    api::mark_running(client, config, codec, id)?;
    let Some(inbound) = codec.decode(&page.to_string())? else {
        return Err(NotionError::Protocol(
            "Notion page input is empty".to_owned(),
        ));
    };
    let outbound = bridge.handle(inbound)?;
    let request = codec.encode(&outbound)?;
    api::send_outbound(client, config, &request)?;
    Ok(())
}

#[derive(Debug, thiserror::Error)]
pub enum NotionError {
    #[error("Notion configuration failed: {0}")]
    Config(String),
    #[error("Notion HTTP request failed")]
    Http(#[source] reqwest::Error),
    #[error(transparent)]
    Channel(#[from] cortexfs_channels::ChannelError),
    #[error(transparent)]
    Bridge(#[from] ChannelBridgeError),
    #[error("Notion response is invalid: {0}")]
    Protocol(String),
}

impl NotionError {
    fn safe_message(&self) -> &'static str {
        match self {
            &Self::Bridge(_) => "Agent execution failed; inspect the runtime audit.",
            &Self::Channel(_) | &Self::Protocol(_) => "Agent channel task failed.",
            &Self::Config(_) | &Self::Http(_) => "Notion channel request failed.",
        }
    }
}
