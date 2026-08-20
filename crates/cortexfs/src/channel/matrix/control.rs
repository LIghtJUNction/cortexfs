use cortexfs_channels::{ChannelActions, ChannelCapabilities, ChannelId};
use reqwest::blocking::Client;

use super::MatrixConfig;
use crate::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig},
};

mod handler;
mod invoke;
mod ops;

pub(super) fn start(
    config: &MatrixConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
) -> Result<ChannelControl, super::MatrixError> {
    host::start(ChannelControlConfig::new(
        ChannelId::from_static("matrix"),
        bridge.clone(),
        ChannelCapabilities {
            send: true,
            tool_control: true,
            polling: true,
            long_polling: true,
            ..ChannelCapabilities::empty()
        },
        ChannelActions {
            reaction: true,
            edit: true,
            delete: true,
            mark_read: true,
            redact: true,
            ..ChannelActions::empty()
        },
        Box::new(handler::Handler::new(client, config)),
    ))
    .map_err(|error| super::MatrixError::Protocol(error.to_string()))
}
