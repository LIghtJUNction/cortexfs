use std::{env, net::SocketAddr, path::PathBuf};
mod disk;
pub use cortexfs::channel::discord::DiscordConfig;
pub use disk::DiscordConfigError;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("usage: cortexfs-channel <discord [--config PATH]|telegram|webhook>")]
    Usage,
    #[error("missing environment variable {0}")]
    Missing(&'static str),
    #[error("invalid environment variable {0}: {1}")]
    Invalid(&'static str, String),
    #[error(transparent)]
    Discord(#[from] DiscordConfigError),
}
#[derive(Clone, Debug)]
pub struct CommonConfig {
    pub socket: PathBuf,
    pub agent: String,
    pub prefix: String,
    pub cwd: Option<String>,
}
#[derive(Debug)]
pub enum CommandConfig {
    Discord {
        config: DiscordConfig,
    },
    Telegram {
        common: CommonConfig,
        token: String,
        api_base: String,
        poll_seconds: u64,
    },
    Webhook {
        common: CommonConfig,
        bind: SocketAddr,
        path: String,
        platform: Platform,
        outbound_url: String,
        token: Option<String>,
    },
}
#[derive(Clone, Copy, Debug)]
pub enum Platform {
    Discord,
    Slack,
    Feishu,
}

pub fn load() -> Result<CommandConfig, ConfigError> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or(ConfigError::Usage)?;
    if command == "discord" {
        return disk::load_command(args);
    }
    if args.next().is_some() {
        return Err(ConfigError::Usage);
    }
    let common = common()?;
    match command.as_str() {
        "telegram" => Ok(CommandConfig::Telegram {
            common,
            token: required("CORTEXFS_TELEGRAM_TOKEN")?,
            api_base: optional("CORTEXFS_TELEGRAM_API_BASE", "https://api.telegram.org"),
            poll_seconds: number("CORTEXFS_TELEGRAM_POLL_SECONDS", 20)?.min(50),
        }),
        "webhook" => Ok(CommandConfig::Webhook {
            common,
            bind: optional("CORTEXFS_CHANNEL_BIND", "127.0.0.1:8765")
                .parse::<SocketAddr>()
                .map_err(|error| {
                    ConfigError::Invalid("CORTEXFS_CHANNEL_BIND", error.to_string())
                })?,
            path: optional("CORTEXFS_CHANNEL_PATH", "/webhook"),
            platform: platform(&required("CORTEXFS_CHANNEL_PLATFORM")?)?,
            outbound_url: required("CORTEXFS_CHANNEL_OUTBOUND_URL")?,
            token: env::var("CORTEXFS_CHANNEL_TOKEN").ok(),
        }),
        _ => Err(ConfigError::Usage),
    }
}

fn common() -> Result<CommonConfig, ConfigError> {
    let agent = required("CORTEXFS_AGENT")?;
    if !cortexfs::is_object_name(&agent) {
        return Err(ConfigError::Invalid(
            "CORTEXFS_AGENT",
            "invalid agent name".to_owned(),
        ));
    }
    let socket = PathBuf::from(required("CORTEXFS_AGENT_SOCKET")?);
    let expected = cortexfs_paths::agent_client_socket(&agent);
    if socket != expected {
        return Err(ConfigError::Invalid(
            "CORTEXFS_AGENT_SOCKET",
            format!("use {}", expected.display()),
        ));
    }
    Ok(CommonConfig {
        socket,
        agent,
        prefix: optional("CORTEXFS_CHANNEL_SESSION_PREFIX", "im"),
        cwd: env::var("CORTEXFS_AGENT_CWD").ok(),
    })
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or(ConfigError::Missing(name))
}

fn optional(name: &'static str, default: &str) -> String {
    env::var(name).unwrap_or_else(|_| default.to_owned())
}

fn number(name: &'static str, default: u64) -> Result<u64, ConfigError> {
    optional(name, &default.to_string())
        .parse::<u64>()
        .map_err(|error| ConfigError::Invalid(name, error.to_string()))
}

fn platform(value: &str) -> Result<Platform, ConfigError> {
    match value {
        "discord" => Ok(Platform::Discord),
        "slack" => Ok(Platform::Slack),
        "feishu" | "lark" => Ok(Platform::Feishu),
        _ => Err(ConfigError::Invalid(
            "CORTEXFS_CHANNEL_PLATFORM",
            value.to_owned(),
        )),
    }
}
