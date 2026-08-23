//! Built-in helpers for process-isolated channel adapters.

use std::{env, path::PathBuf, time::Duration};

use cortexfs_channels::ChannelId;

use crate::ChannelSdkError;

const DEFAULT_REPLY_TIMEOUT_SECS: u64 = 600;

/// Launch settings shared by custom channel adapters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DriverLaunchConfig {
    /// Complete channel instance id such as `telegram.primary`.
    pub channel_id: ChannelId,
    /// Runtime-owned driver socket path.
    pub socket: PathBuf,
    /// Prefix used for generated request ids.
    pub request_prefix: String,
    /// Timeout for correlated driver operations.
    pub reply_timeout: Duration,
}

impl DriverLaunchConfig {
    /// Loads launch settings from the standard adapter environment.
    pub fn from_env() -> Result<Self, ChannelSdkError> {
        let channel = required_env("CORTEXFS_CHANNEL_ID").or_else(|_error| {
            required_env("CTX_CHANNEL_ID").map(|value| value.replace('/', "."))
        })?;
        let channel_id = ChannelId::new(&channel)
            .map_err(|_error| ChannelSdkError::config("channel id is invalid"))?;
        let socket = env::var("CORTEXFS_CHANNEL_SOCKET")
            .or_else(|_error| env::var("CTX_CHANNEL_SOCKET"))
            .map_or_else(
                |_error| cortexfs_paths::channel_driver_socket(channel_id.as_str()),
                PathBuf::from,
            );
        let request_prefix = env::var("CORTEXFS_CHANNEL_REQUEST_PREFIX")
            .unwrap_or_else(|_error| format!("{}-", channel_id.as_str()));
        let reply_timeout = env::var("CORTEXFS_CHANNEL_REPLY_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_REPLY_TIMEOUT_SECS)
            .clamp(1, 3600);
        Ok(Self {
            channel_id,
            socket,
            request_prefix,
            reply_timeout: Duration::from_secs(reply_timeout),
        })
    }
}

fn required_env(name: &str) -> Result<String, ChannelSdkError> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ChannelSdkError::config(format!("{name} is required")))
}
