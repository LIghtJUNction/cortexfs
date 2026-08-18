use std::error::Error;

use super::super::config::DiscordConfig;
use cortexfs::channel::{bridge::AgentChannelBridge, discord};
use cortexfs_channels::ChannelSessionRoute;

pub(super) fn run(config: &DiscordConfig) -> Result<(), Box<dyn Error>> {
    let route =
        ChannelSessionRoute::new(&config.agent, &config.session_prefix)?.with_identity_isolation();
    let bridge = match config.channel.clone() {
        Some(channel) => AgentChannelBridge::new_with_channel(
            config.agent_socket.clone(),
            route,
            config.cwd.clone(),
            channel,
        ),
        None => AgentChannelBridge::new(config.agent_socket.clone(), route, config.cwd.clone()),
    };
    bridge.check_socket()?;
    discord::run(config, &bridge)?;
    Ok(())
}
