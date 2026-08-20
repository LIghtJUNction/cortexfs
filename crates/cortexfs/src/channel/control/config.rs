use std::{fmt, path::PathBuf};

use cortexfs_channels::{ChannelActions, ChannelCapabilities, ChannelId};

use super::ChannelControlHandler;
use crate::channel::bridge::AgentChannelBridge;

pub struct ChannelControlConfig {
    pub(crate) channel: ChannelId,
    pub(crate) socket: PathBuf,
    pub(crate) bridge: AgentChannelBridge,
    pub(crate) capabilities: ChannelCapabilities,
    pub(crate) actions: ChannelActions,
    pub(crate) handler: Box<dyn ChannelControlHandler>,
}

impl fmt::Debug for ChannelControlConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelControlConfig")
            .field("channel", &self.channel)
            .field("socket", &self.socket)
            .finish_non_exhaustive()
    }
}

impl ChannelControlConfig {
    #[must_use]
    pub fn new(
        channel: ChannelId,
        bridge: AgentChannelBridge,
        capabilities: ChannelCapabilities,
        actions: ChannelActions,
        handler: Box<dyn ChannelControlHandler>,
    ) -> Self {
        Self {
            socket: cortexfs_paths::channel_driver_socket(channel.as_str()),
            channel,
            bridge,
            capabilities,
            actions,
            handler,
        }
    }
}
