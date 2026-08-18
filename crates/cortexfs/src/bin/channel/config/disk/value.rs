use std::path::PathBuf;

use serde::Deserialize;

use cortexfs::channel::discord::DiscordConfig;
use cortexfs_channels::ChannelId;

const DEFAULT_API_BASE: &str = "https://discord.com/api/v10";
const DEFAULT_GATEWAY: &str = "wss://gateway.discord.gg/?v=10&encoding=json";
const DEFAULT_INTENTS: u64 = 1 | (1 << 9) | (1 << 12) | (1 << 15);

#[derive(Deserialize)]
pub(super) struct RawDiscordConfig {
    application_id: String,
    bot_token: String,
    agent_socket: PathBuf,
    agent: String,
    #[serde(default = "default_prefix")]
    session_prefix: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default)]
    channel: Option<ChannelId>,
    #[serde(default = "default_api_base")]
    api_base: String,
    #[serde(default = "default_gateway")]
    gateway_url: String,
    #[serde(default = "default_intents")]
    intents: u64,
}

impl RawDiscordConfig {
    pub(super) fn into_config(self) -> DiscordConfig {
        DiscordConfig {
            application_id: self.application_id,
            bot_token: self.bot_token,
            agent_socket: self.agent_socket,
            agent: self.agent,
            session_prefix: self.session_prefix,
            cwd: self.cwd,
            channel: self.channel,
            api_base: self.api_base,
            gateway_url: self.gateway_url,
            intents: self.intents,
        }
    }
}

fn default_prefix() -> String {
    "discord".to_owned()
}
fn default_api_base() -> String {
    DEFAULT_API_BASE.to_owned()
}
fn default_gateway() -> String {
    DEFAULT_GATEWAY.to_owned()
}
fn default_intents() -> u64 {
    DEFAULT_INTENTS
}
