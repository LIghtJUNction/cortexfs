use cortexfs_channels::{ChannelCapabilities, ChannelId};
use reqwest::blocking::Client;

use super::TwitterConfig;
use crate::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig, CodecHandler},
};

mod ops;
mod transport;

pub(super) fn start(
    config: &TwitterConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
    bot_id: &str,
) -> Result<ChannelControl, super::TwitterError> {
    host::start(ChannelControlConfig::new(
        ChannelId::from_static("twitter"),
        bridge.clone(),
        ChannelCapabilities {
            send: true,
            tool_control: true,
            polling: true,
            ..ChannelCapabilities::empty()
        },
        cortexfs_channels::ChannelActions::empty(),
        Box::new(CodecHandler::new(Box::new(transport::Transport::new(
            client, config, bot_id,
        )))),
    ))
    .map_err(|error| super::TwitterError::Protocol(error.to_string()))
}
