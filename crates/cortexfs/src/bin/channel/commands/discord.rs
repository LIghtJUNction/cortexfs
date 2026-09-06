use std::error::Error;

use super::super::config::DiscordConfig;
use cortexfs::channel::{bridge::AgentChannelBridge, discord};

pub(super) fn run(config: &DiscordConfig) -> Result<(), Box<dyn Error>> {
    let route = super::common::route(
        &config.agent,
        &config.session_prefix,
        config.allowed_senders.clone(),
    )?;
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
