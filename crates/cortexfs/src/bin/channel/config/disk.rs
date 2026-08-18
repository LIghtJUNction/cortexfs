use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

use super::{CommandConfig, ConfigError};
use cortexfs::channel::discord::DiscordConfig;
mod value;

const MAX_CONFIG_BYTES: u64 = 16 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum DiscordConfigError {
    #[error("cannot inspect Discord config {path}: {source}")]
    Inspect {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("Discord config must be a regular owner-only file: {0}")]
    UnsafeFile(PathBuf),
    #[error("cannot read Discord config {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid Discord config: {0}")]
    Parse(String),
}

pub(super) fn load(path: &Path) -> Result<DiscordConfig, DiscordConfigError> {
    let metadata = fs::symlink_metadata(path).map_err(|source| DiscordConfigError::Inspect {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() || metadata.mode() & 0o077 != 0 || metadata.len() > MAX_CONFIG_BYTES {
        return Err(DiscordConfigError::UnsafeFile(path.to_owned()));
    }
    let text = fs::read_to_string(path).map_err(|source| DiscordConfigError::Read {
        path: path.to_owned(),
        source,
    })?;
    let raw: value::RawDiscordConfig =
        toml::from_str(&text).map_err(|error| DiscordConfigError::Parse(error.to_string()))?;
    let config = raw.into_config();
    validate(&config)?;
    Ok(config)
}

fn validate(config: &DiscordConfig) -> Result<(), DiscordConfigError> {
    let fields = [
        ("application_id", config.application_id.as_str()),
        ("bot_token", config.bot_token.as_str()),
        ("agent", config.agent.as_str()),
        ("session_prefix", config.session_prefix.as_str()),
        ("api_base", config.api_base.as_str()),
        ("gateway_url", config.gateway_url.as_str()),
    ];
    if fields
        .iter()
        .any(|&(_, value)| value.is_empty() || value.contains('\0'))
        || !cortexfs::is_object_name(&config.agent)
        || !config
            .application_id
            .bytes()
            .all(|byte| byte.is_ascii_digit())
        || config.intents == 0
    {
        return Err(DiscordConfigError::Parse(
            "required values are invalid".to_owned(),
        ));
    }
    let expected = cortexfs_paths::agent_client_socket(&config.agent);
    if config.agent_socket != expected {
        return Err(DiscordConfigError::Parse(format!(
            "agent_socket must be {}",
            expected.display()
        )));
    }
    if config
        .channel
        .as_ref()
        .is_some_and(|channel| channel.family() != "discord")
    {
        return Err(DiscordConfigError::Parse(
            "channel must use the discord family".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn load_command(
    mut args: impl Iterator<Item = String>,
) -> Result<CommandConfig, ConfigError> {
    let mut path = cortexfs_paths::channel_config_path("discord");
    while let Some(arg) = args.next() {
        if arg != "--config" {
            return Err(ConfigError::Usage);
        }
        path = PathBuf::from(args.next().ok_or(ConfigError::Usage)?);
    }
    Ok(CommandConfig::Discord {
        config: load(&path)?,
    })
}
