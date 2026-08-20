use cortexfs_channels::{ChannelActions, ChannelCapabilities, ChannelId};
use reqwest::blocking::Client;

use super::{DiscordConfig, DiscordError};
use crate::channel::{
    bridge::AgentChannelBridge,
    control::{self as host, ChannelControl, ChannelControlConfig, ChannelControlError},
};

mod handler;

pub(super) type Control = ChannelControl;

pub(super) fn start(
    config: &DiscordConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
) -> Result<Control, DiscordError> {
    let channel = config
        .channel
        .clone()
        .unwrap_or_else(|| ChannelId::from_static("discord"));
    host::start(ChannelControlConfig::new(
        channel,
        bridge.clone(),
        ChannelCapabilities {
            send: true,
            tool_control: true,
            websocket: true,
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
    .map_err(Into::into)
}

impl From<ChannelControlError> for DiscordError {
    fn from(error: ChannelControlError) -> Self {
        match error {
            ChannelControlError::Connection(error) => Self::Driver(error),
            ChannelControlError::Driver(error) => Self::Runtime(error),
            ChannelControlError::Operation(reason) => Self::Protocol(reason),
            ChannelControlError::Stopped => Self::Protocol("Discord control stopped".to_owned()),
        }
    }
}
