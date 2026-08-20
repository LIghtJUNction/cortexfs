use cortexfs_channels::{ChannelActions, ChannelCapabilities, ChannelId};
use reqwest::blocking::Client;

use super::TelegramConfig;
use crate::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig},
};

mod handler;
mod invoke;

pub(super) fn start(
    config: &TelegramConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
) -> Result<ChannelControl, super::TelegramError> {
    host::start(ChannelControlConfig::new(
        ChannelId::from_static("telegram"),
        bridge.clone(),
        ChannelCapabilities {
            send: true,
            tool_control: true,
            polling: true,
            long_polling: true,
            ..ChannelCapabilities::empty()
        },
        ChannelActions {
            typing: true,
            preview: true,
            reaction: true,
            edit: true,
            delete: true,
            pin: true,
            unpin: true,
            redact: true,
            ..ChannelActions::empty()
        },
        Box::new(handler::Handler::new(client, config)),
    ))
    .map_err(|error| super::TelegramError::Api(error.to_string()))
}
