#![forbid(unsafe_code)]

pub mod config;
pub mod webhook;

use std::io::Write as _;
use std::process::ExitCode;

use config::CommandConfig;
use cortexfs::channel::bridge::AgentChannelBridge;
use cortexfs_channels::ChannelSessionRoute;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ignored = writeln!(std::io::stderr(), "cortexfs-channel: {error}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    match config::load()? {
        CommandConfig::Telegram {
            common,
            token,
            api_base,
            poll_seconds,
        } => {
            let route = ChannelSessionRoute::new(&common.agent, &common.prefix)?;
            let bridge = AgentChannelBridge::new(common.socket, route, common.cwd);
            let config = cortexfs::channel::telegram::TelegramConfig::new(token, api_base)?
                .with_poll_seconds(poll_seconds);
            cortexfs::channel::telegram::run(&config, &bridge)?;
        }
        CommandConfig::Webhook {
            common,
            bind,
            path,
            platform,
            outbound_url,
            token,
        } => {
            let route = ChannelSessionRoute::new(&common.agent, &common.prefix)?;
            let bridge = AgentChannelBridge::new(common.socket, route, common.cwd);
            let config = webhook::WebhookConfig {
                bind,
                path,
                platform,
                outbound_url,
                token,
            };
            webhook::run(&config, &bridge)?;
        }
    }
    Ok(())
}
