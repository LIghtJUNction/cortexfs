use std::error::Error;
use std::io::Write as _;

use super::super::config::CommonConfig;
use cortexfs::channel::bridge::AgentChannelBridge;
use cortexfs_channels::{ChannelError, ChannelSessionRoute};

pub(super) fn bridge(common: CommonConfig) -> Result<AgentChannelBridge, Box<dyn Error>> {
    let route = route(&common.agent, &common.prefix, common.allowed_senders)?;
    let bridge = match common.channel {
        Some(channel) => {
            AgentChannelBridge::new_with_channel(common.socket, route, common.cwd, channel)
        }
        None => AgentChannelBridge::new(common.socket, route, common.cwd),
    };
    bridge.check_socket()?;
    Ok(bridge)
}

pub(super) fn route(
    agent: &str,
    prefix: &str,
    allowed_senders: Vec<String>,
) -> Result<ChannelSessionRoute, ChannelError> {
    if allowed_senders.is_empty() {
        let _ignored = writeln!(
            std::io::stderr(),
            "cortexfs-channel: all senders denied; configure CORTEXFS_CHANNEL_ALLOWED_SENDERS or Discord allowed_senders"
        );
    }
    Ok(ChannelSessionRoute::new(agent, prefix)?
        .with_identity_isolation()
        .with_allowed_senders(allowed_senders))
}
