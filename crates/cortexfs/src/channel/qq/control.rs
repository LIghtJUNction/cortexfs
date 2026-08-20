use cortexfs_channels::{ChannelCapabilities, ChannelId};
use reqwest::blocking::Client;

use super::QqConfig;
use crate::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig},
};

mod handler;
mod invoke;

pub(super) fn start(
    config: &QqConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
) -> Result<ChannelControl, super::QqError> {
    host::start(ChannelControlConfig::new(
        ChannelId::from_static("qq"),
        bridge.clone(),
        ChannelCapabilities {
            send: true,
            tool_control: true,
            websocket: true,
            group: true,
            ..ChannelCapabilities::empty()
        },
        cortexfs_channels::ChannelActions::empty(),
        Box::new(handler::Handler::new(client, config)),
    ))
    .map_err(|error| super::QqError::Protocol(error.to_string()))
}
