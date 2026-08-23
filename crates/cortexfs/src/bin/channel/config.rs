use cortexfs::channel::email::EmailConfig;
use cortexfs_channels::{ChannelId, ChannelProgressPolicy};
use std::{env, net::SocketAddr, path::PathBuf};
mod disk;
pub use cortexfs::channel::discord::DiscordConfig;
pub use disk::DiscordConfigError;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error(
        "usage: cortexfs-channel <list|show FAMILY|preset FAMILY|discord [--config PATH]|telegram|bluesky|dingtalk|matrix|mattermost|qq|reddit|gmail|email|irc|twitch|twitter|mochat|notion|signal|webhook|web|driver>"
    )]
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
    pub channel: Option<ChannelId>,
    pub progress: ChannelProgressPolicy,
}
#[derive(Debug)]
pub enum CatalogAction {
    List,
    Show { family: String },
    Preset { family: String },
}

#[derive(Debug)]
pub enum CommandConfig {
    Catalog(CatalogAction),
    Discord {
        config: DiscordConfig,
    },
    Telegram {
        common: CommonConfig,
        token: String,
        api_base: String,
        poll_seconds: u64,
    },
    Bluesky {
        common: CommonConfig,
        handle: String,
        app_password: String,
        api_base: String,
        poll_seconds: u64,
    },
    DingTalk {
        common: CommonConfig,
        client_id: String,
        client_secret: String,
        gateway_url: String,
    },
    Matrix {
        common: CommonConfig,
        homeserver: String,
        access_token: String,
        rooms: Vec<String>,
        sync_seconds: u64,
    },
    Mattermost {
        common: CommonConfig,
        base_url: String,
        token: String,
        channels: Vec<String>,
        reconnect_seconds: u64,
    },
    Qq {
        common: CommonConfig,
        config: cortexfs::channel::qq::QqConfig,
    },
    Reddit {
        common: CommonConfig,
        config: cortexfs::channel::reddit::RedditConfig,
    },
    Irc {
        common: CommonConfig,
        server: String,
        port: u16,
        nickname: String,
        channels: Vec<String>,
        password: Option<String>,
    },
    Twitch {
        common: CommonConfig,
        config: cortexfs::channel::twitch::TwitchConfig,
    },
    Twitter {
        common: CommonConfig,
        config: cortexfs::channel::twitter::TwitterConfig,
    },
    Mochat {
        common: CommonConfig,
        config: cortexfs::channel::mochat::MochatConfig,
    },
    Notion {
        common: CommonConfig,
        config: cortexfs::channel::notion::NotionConfig,
    },
    Signal {
        common: CommonConfig,
        account: String,
        executable: String,
    },
    Gmail {
        common: CommonConfig,
        bind: SocketAddr,
        path: String,
        access_token: String,
        api_base: String,
        token: Option<String>,
    },
    Email {
        common: CommonConfig,
        config: EmailConfig,
    },
    Webhook {
        common: CommonConfig,
        bind: SocketAddr,
        path: String,
        platform: Platform,
        outbound_url: String,
        token: Option<String>,
        verify_token: Option<String>,
    },
    Web {
        common: CommonConfig,
        bind: SocketAddr,
        path: String,
        token: Option<String>,
    },
    Driver {
        common: CommonConfig,
        channel: ChannelId,
        socket: PathBuf,
    },
}
#[derive(Clone, Copy, Debug)]
pub enum Platform {
    Discord,
    Slack,
    Feishu,
    Line,
    Nextcloud,
    Teams,
    Linq,
    WhatsApp,
    WeCom,
}

pub fn load() -> Result<CommandConfig, ConfigError> {
    let mut args = env::args().skip(1);
    let command = args.next().ok_or(ConfigError::Usage)?;
    if matches!(command.as_str(), "list" | "show" | "preset") {
        return super::catalog::load_action(&command, args);
    }
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
        "bluesky" => bluesky_config(common),
        "dingtalk" => Ok(CommandConfig::DingTalk {
            common,
            client_id: required("CORTEXFS_DINGTALK_CLIENT_ID")?,
            client_secret: required("CORTEXFS_DINGTALK_CLIENT_SECRET")?,
            gateway_url: optional(
                "CORTEXFS_DINGTALK_GATEWAY_URL",
                "https://api.dingtalk.com/v1.0/gateway/connections/open",
            ),
        }),
        "matrix" => Ok(CommandConfig::Matrix {
            common,
            homeserver: required("CORTEXFS_MATRIX_HOMESERVER")?,
            access_token: required("CORTEXFS_MATRIX_ACCESS_TOKEN")?,
            rooms: list("CORTEXFS_MATRIX_ROOMS"),
            sync_seconds: number("CORTEXFS_MATRIX_SYNC_SECONDS", 30)?.min(50),
        }),
        "mattermost" => mattermost_config(common),
        "qq" => qq_config(common),
        "reddit" => reddit_config(common),
        "gmail" => Ok(CommandConfig::Gmail {
            common,
            bind: optional("CORTEXFS_GMAIL_BIND", "127.0.0.1:8767")
                .parse::<SocketAddr>()
                .map_err(|error| ConfigError::Invalid("CORTEXFS_GMAIL_BIND", error.to_string()))?,
            path: optional("CORTEXFS_GMAIL_PATH", "/gmail/push"),
            access_token: required("CORTEXFS_GMAIL_ACCESS_TOKEN")?,
            api_base: optional(
                "CORTEXFS_GMAIL_API_BASE",
                "https://gmail.googleapis.com/gmail/v1",
            ),
            token: env::var("CORTEXFS_GMAIL_PUBSUB_TOKEN")
                .ok()
                .filter(|value| !value.is_empty()),
        }),
        "email" => email_config(common),
        "irc" => irc_config(common),
        "twitch" => twitch_config(common),
        "twitter" => twitter_config(common),
        "mochat" => mochat_config(common),
        "notion" => notion_config(common),
        "signal" => signal_config(common),
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
            verify_token: env::var("CORTEXFS_CHANNEL_VERIFY_TOKEN").ok(),
        }),
        "web" => {
            let bind = optional("CORTEXFS_WEB_BIND", "127.0.0.1:8766")
                .parse::<SocketAddr>()
                .map_err(|error| ConfigError::Invalid("CORTEXFS_WEB_BIND", error.to_string()))?;
            let path = optional("CORTEXFS_WEB_PATH", "/v1/interaction");
            if !path.starts_with('/') {
                return Err(ConfigError::Invalid(
                    "CORTEXFS_WEB_PATH",
                    "path must start with /".to_owned(),
                ));
            }
            let token = env::var("CORTEXFS_WEB_TOKEN")
                .ok()
                .filter(|value| !value.is_empty());
            if !bind.ip().is_loopback() && token.is_none() {
                return Err(ConfigError::Invalid(
                    "CORTEXFS_WEB_TOKEN",
                    "required when CORTEXFS_WEB_BIND is not loopback".to_owned(),
                ));
            }
            Ok(CommandConfig::Web {
                common,
                bind,
                path,
                token,
            })
        }
        "driver" => {
            let channel = ChannelId::new(required("CORTEXFS_CHANNEL_ID")?)
                .map_err(|error| ConfigError::Invalid("CORTEXFS_CHANNEL_ID", error.to_string()))?;
            let expected = cortexfs_paths::channel_driver_socket(channel.as_str());
            let socket = PathBuf::from(optional(
                "CORTEXFS_CHANNEL_SOCKET",
                &expected.display().to_string(),
            ));
            if socket != expected {
                return Err(ConfigError::Invalid(
                    "CORTEXFS_CHANNEL_SOCKET",
                    format!("use {}", expected.display()),
                ));
            }
            Ok(CommandConfig::Driver {
                common,
                channel,
                socket,
            })
        }
        _ => Err(ConfigError::Usage),
    }
}

fn mattermost_config(common: CommonConfig) -> Result<CommandConfig, ConfigError> {
    Ok(CommandConfig::Mattermost {
        common,
        base_url: required("CORTEXFS_MATTERMOST_URL")?,
        token: required("CORTEXFS_MATTERMOST_TOKEN")?,
        channels: list("CORTEXFS_MATTERMOST_CHANNELS"),
        reconnect_seconds: number("CORTEXFS_MATTERMOST_RECONNECT_SECONDS", 5)?.min(300),
    })
}

fn bluesky_config(common: CommonConfig) -> Result<CommandConfig, ConfigError> {
    Ok(CommandConfig::Bluesky {
        common,
        handle: required("CORTEXFS_BLUESKY_HANDLE")?,
        app_password: required("CORTEXFS_BLUESKY_APP_PASSWORD")?,
        api_base: optional("CORTEXFS_BLUESKY_API_BASE", "https://bsky.social/xrpc"),
        poll_seconds: number("CORTEXFS_BLUESKY_POLL_SECONDS", 5)?.min(300),
    })
}

fn qq_config(common: CommonConfig) -> Result<CommandConfig, ConfigError> {
    let config = cortexfs::channel::qq::QqConfig::new(
        required("CORTEXFS_QQ_APP_ID")?,
        required("CORTEXFS_QQ_TOKEN")?,
        optional("CORTEXFS_QQ_API_BASE", "https://api.sgroup.qq.com"),
        optional(
            "CORTEXFS_QQ_GATEWAY_URL",
            "https://api.sgroup.qq.com/gateway",
        ),
    )
    .map_err(|error| ConfigError::Invalid("CORTEXFS_QQ_CONFIG", error.to_string()))?
    .with_intents(number("CORTEXFS_QQ_INTENTS", (1 << 25) | (1 << 30))?)
    .with_reconnect_seconds(number("CORTEXFS_QQ_RECONNECT_SECONDS", 5)?.min(300));
    Ok(CommandConfig::Qq { common, config })
}

fn irc_config(common: CommonConfig) -> Result<CommandConfig, ConfigError> {
    let port = u16::try_from(number("CORTEXFS_IRC_PORT", 6667)?)
        .map_err(|error| ConfigError::Invalid("CORTEXFS_IRC_PORT", error.to_string()))?;
    Ok(CommandConfig::Irc {
        common,
        server: required("CORTEXFS_IRC_SERVER")?,
        port,
        nickname: required("CORTEXFS_IRC_NICKNAME")?,
        channels: list("CORTEXFS_IRC_CHANNELS"),
        password: env::var("CORTEXFS_IRC_PASSWORD")
            .ok()
            .filter(|value| !value.is_empty()),
    })
}

fn reddit_config(common: CommonConfig) -> Result<CommandConfig, ConfigError> {
    Ok(CommandConfig::Reddit {
        common,
        config: cortexfs::channel::reddit::RedditConfig {
            client_id: required("CORTEXFS_REDDIT_CLIENT_ID")?,
            client_secret: required("CORTEXFS_REDDIT_CLIENT_SECRET")?,
            refresh_token: required("CORTEXFS_REDDIT_REFRESH_TOKEN")?,
            username: required("CORTEXFS_REDDIT_USERNAME")?,
            subreddits: list("CORTEXFS_REDDIT_SUBREDDITS"),
            api_base: optional("CORTEXFS_REDDIT_API_BASE", "https://oauth.reddit.com"),
            token_url: optional(
                "CORTEXFS_REDDIT_TOKEN_URL",
                "https://www.reddit.com/api/v1/access_token",
            ),
            poll_seconds: number("CORTEXFS_REDDIT_POLL_SECONDS", 5)?.min(300),
        },
    })
}

fn signal_config(common: CommonConfig) -> Result<CommandConfig, ConfigError> {
    Ok(CommandConfig::Signal {
        common,
        account: required("CORTEXFS_SIGNAL_ACCOUNT")?,
        executable: optional("CORTEXFS_SIGNAL_CLI", "signal-cli"),
    })
}

fn twitch_config(common: CommonConfig) -> Result<CommandConfig, ConfigError> {
    let channels = list("CORTEXFS_TWITCH_CHANNELS")
        .into_iter()
        .filter_map(|channel| cortexfs_channels::platform::twitch::normalize_channel(&channel))
        .collect::<Vec<_>>();
    if channels.is_empty() {
        return Err(ConfigError::Invalid(
            "CORTEXFS_TWITCH_CHANNELS",
            "at least one channel is required".to_owned(),
        ));
    }
    Ok(CommandConfig::Twitch {
        common,
        config: cortexfs::channel::twitch::TwitchConfig {
            server: optional("CORTEXFS_TWITCH_SERVER", "irc.chat.twitch.tv"),
            port: port("CORTEXFS_TWITCH_PORT", 6697)?,
            nickname: required("CORTEXFS_TWITCH_USERNAME")?,
            oauth_token: required("CORTEXFS_TWITCH_OAUTH_TOKEN")?,
            channels,
            mention_only: boolean("CORTEXFS_TWITCH_MENTION_ONLY", false)?,
        },
    })
}

fn twitter_config(common: CommonConfig) -> Result<CommandConfig, ConfigError> {
    let config = cortexfs::channel::twitter::TwitterConfig::new(
        required("CORTEXFS_TWITTER_BEARER_TOKEN")?,
        optional("CORTEXFS_TWITTER_API_BASE", "https://api.x.com/2"),
    )
    .map_err(|error| ConfigError::Invalid("CORTEXFS_TWITTER_CONFIG", error.to_string()))?
    .with_allowed_users(list("CORTEXFS_TWITTER_ALLOWED_USERS"))
    .with_poll_seconds(number("CORTEXFS_TWITTER_POLL_SECONDS", 15)?.min(300));
    Ok(CommandConfig::Twitter { common, config })
}

fn mochat_config(common: CommonConfig) -> Result<CommandConfig, ConfigError> {
    let config = cortexfs::channel::mochat::MochatConfig::new(
        required("CORTEXFS_MOCHAT_API_BASE")?,
        required("CORTEXFS_MOCHAT_API_TOKEN")?,
    )
    .map_err(|error| ConfigError::Invalid("CORTEXFS_MOCHAT_CONFIG", error.to_string()))?
    .with_allowed_users(list("CORTEXFS_MOCHAT_ALLOWED_USERS"))
    .with_poll_seconds(number("CORTEXFS_MOCHAT_POLL_SECONDS", 5)?.min(300));
    Ok(CommandConfig::Mochat { common, config })
}

fn notion_config(common: CommonConfig) -> Result<CommandConfig, ConfigError> {
    let max_concurrent =
        usize::try_from(number("CORTEXFS_NOTION_MAX_CONCURRENT", 1)?).map_err(|error| {
            ConfigError::Invalid("CORTEXFS_NOTION_MAX_CONCURRENT", error.to_string())
        })?;
    let config = cortexfs::channel::notion::NotionConfig::new(
        optional("CORTEXFS_NOTION_API_BASE", "https://api.notion.com/v1"),
        required("CORTEXFS_NOTION_API_TOKEN")?,
        required("CORTEXFS_NOTION_DATABASE_ID")?,
    )
    .map_err(|error| ConfigError::Invalid("CORTEXFS_NOTION_CONFIG", error.to_string()))?
    .with_properties(
        optional("CORTEXFS_NOTION_STATUS_PROPERTY", "Status"),
        optional("CORTEXFS_NOTION_INPUT_PROPERTY", "Input"),
        optional("CORTEXFS_NOTION_RESULT_PROPERTY", "Result"),
    )
    .with_status_type(optional("CORTEXFS_NOTION_STATUS_TYPE", "auto"))
    .with_poll_seconds(number("CORTEXFS_NOTION_POLL_SECONDS", 5)?.min(300))
    .with_max_concurrent(max_concurrent)
    .with_recover_stale(boolean("CORTEXFS_NOTION_RECOVER_STALE", true)?);
    Ok(CommandConfig::Notion { common, config })
}

fn email_config(common: CommonConfig) -> Result<CommandConfig, ConfigError> {
    let imap_port = port("CORTEXFS_EMAIL_IMAP_PORT", 993)?;
    let smtp_port = port("CORTEXFS_EMAIL_SMTP_PORT", 587)?;
    let username = required("CORTEXFS_EMAIL_USERNAME")?;
    Ok(CommandConfig::Email {
        common,
        config: EmailConfig {
            imap_host: required("CORTEXFS_EMAIL_IMAP_HOST")?,
            imap_port,
            smtp_host: required("CORTEXFS_EMAIL_SMTP_HOST")?,
            smtp_port,
            username: username.clone(),
            password: required("CORTEXFS_EMAIL_PASSWORD")?,
            from: env::var("CORTEXFS_EMAIL_FROM").unwrap_or(username),
            mailbox: optional("CORTEXFS_EMAIL_MAILBOX", "INBOX"),
            idle_seconds: number("CORTEXFS_EMAIL_IDLE_SECONDS", 60)?.min(1_740),
        },
    })
}

fn port(name: &'static str, default: u64) -> Result<u16, ConfigError> {
    u16::try_from(number(name, default)?)
        .map_err(|error| ConfigError::Invalid(name, error.to_string()))
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
        channel: env::var("CORTEXFS_CHANNEL_ID")
            .ok()
            .filter(|value| !value.is_empty())
            .map(|value| {
                ChannelId::new(value)
                    .map_err(|error| ConfigError::Invalid("CORTEXFS_CHANNEL_ID", error.to_string()))
            })
            .transpose()?,
        progress: progress()?,
    })
}

fn progress() -> Result<ChannelProgressPolicy, ConfigError> {
    let edit_chunk_bytes = optional_usize("CORTEXFS_CHANNEL_PROGRESS_EDIT_CHUNK_BYTES")?;
    Ok(ChannelProgressPolicy {
        reaction: configured("CORTEXFS_CHANNEL_PROGRESS_REACTION"),
        error_reaction: configured("CORTEXFS_CHANNEL_PROGRESS_ERROR_REACTION"),
        placeholder: configured("CORTEXFS_CHANNEL_PROGRESS_PLACEHOLDER"),
        error_prefix: configured("CORTEXFS_CHANNEL_PROGRESS_ERROR_PREFIX"),
        typing: optional_bool("CORTEXFS_CHANNEL_PROGRESS_TYPING")?,
        edit_interval_ms: optional_number("CORTEXFS_CHANNEL_PROGRESS_EDIT_INTERVAL_MS")?,
        edit_chunk_bytes,
    })
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    configured(name).ok_or(ConfigError::Missing(name))
}

fn optional(name: &'static str, default: &str) -> String {
    configured(name).unwrap_or_else(|| default.to_owned())
}

fn number(name: &'static str, default: u64) -> Result<u64, ConfigError> {
    optional(name, &default.to_string())
        .parse::<u64>()
        .map_err(|error| ConfigError::Invalid(name, error.to_string()))
}

fn configured(name: &'static str) -> Option<String> {
    env::var(name).ok().filter(|value| !value.is_empty())
}

fn optional_number(name: &'static str) -> Result<Option<u64>, ConfigError> {
    configured(name)
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|error| ConfigError::Invalid(name, error.to_string()))
        })
        .transpose()
}

fn optional_usize(name: &'static str) -> Result<Option<usize>, ConfigError> {
    configured(name)
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|error| ConfigError::Invalid(name, error.to_string()))
        })
        .transpose()
}

fn optional_bool(name: &'static str) -> Result<bool, ConfigError> {
    configured(name)
        .map(|value| {
            value
                .parse::<bool>()
                .map_err(|error| ConfigError::Invalid(name, error.to_string()))
        })
        .transpose()
        .map(|value| value.unwrap_or(false))
}

fn boolean(name: &'static str, default: bool) -> Result<bool, ConfigError> {
    match optional(name, if default { "true" } else { "false" })
        .to_ascii_lowercase()
        .as_str()
    {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        value => Err(ConfigError::Invalid(name, value.to_owned())),
    }
}

fn list(name: &'static str) -> Vec<String> {
    optional(name, "")
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn platform(value: &str) -> Result<Platform, ConfigError> {
    match value {
        "discord" => Ok(Platform::Discord),
        "slack" => Ok(Platform::Slack),
        "feishu" | "lark" => Ok(Platform::Feishu),
        "whatsapp" => Ok(Platform::WhatsApp),
        "line" => Ok(Platform::Line),
        "nextcloud" | "nextcloud-talk" => Ok(Platform::Nextcloud),
        "teams" | "microsoft-teams" => Ok(Platform::Teams),
        "linq" => Ok(Platform::Linq),
        "wecom" | "wechat-work" => Ok(Platform::WeCom),
        _ => Err(ConfigError::Invalid(
            "CORTEXFS_CHANNEL_PLATFORM",
            value.to_owned(),
        )),
    }
}
