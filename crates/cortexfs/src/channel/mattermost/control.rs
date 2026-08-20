use cortexfs_channels::{ChannelActions, ChannelCapabilities, ChannelId};
use reqwest::blocking::Client;

use super::MattermostConfig;
use crate::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig},
};

mod handler;
mod invoke;
mod upload;

pub(super) fn start(
    config: &MattermostConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
) -> Result<ChannelControl, super::MattermostError> {
    host::start(ChannelControlConfig::new(
        ChannelId::from_static("mattermost"),
        bridge.clone(),
        ChannelCapabilities {
            send: true,
            tool_control: true,
            websocket: true,
            ..ChannelCapabilities::empty()
        },
        ChannelActions {
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
    .map_err(|error| super::MattermostError::Protocol(error.to_string()))
}
