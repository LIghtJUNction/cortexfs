use std::error::Error;

use super::super::config::CommonConfig;
use cortexfs::channel::bridge::AgentChannelBridge;
use cortexfs_channels::ChannelSessionRoute;

pub(super) fn bridge(common: CommonConfig) -> Result<AgentChannelBridge, Box<dyn Error>> {
    let route = ChannelSessionRoute::new(&common.agent, &common.prefix)?.with_identity_isolation();
    let bridge = match common.channel {
        Some(channel) => {
            AgentChannelBridge::new_with_channel(common.socket, route, common.cwd, channel)
        }
        None => AgentChannelBridge::new(common.socket, route, common.cwd),
    };
    bridge.check_socket()?;
    Ok(bridge)
}
