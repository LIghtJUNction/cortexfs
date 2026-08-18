#![expect(
    clippy::field_scoped_visibility_modifiers,
    clippy::redundant_pub_crate,
    reason = "configuration is shared by private driver modules"
)]

use std::{env, path::PathBuf, time::Duration};

use crate::error::{Error, Result};

pub(crate) struct Config {
    pub(crate) app_token: String,
    pub(crate) bot_token: String,
    pub(crate) api_base: String,
    pub(crate) socket: PathBuf,
    pub(crate) reconnect_seconds: u64,
    pub(crate) reply_timeout: Duration,
}

impl Config {
    pub(crate) fn load() -> Result<Self> {
        let channel = "slack";
        let expected = cortexfs_paths::channel_driver_socket(channel);
        let socket = PathBuf::from(optional(
            "CORTEXFS_CHANNEL_SOCKET",
            &expected.display().to_string(),
        ));
        if socket != expected {
            return Err(Error::Config(format!(
                "CORTEXFS_CHANNEL_SOCKET must be {}",
                expected.display()
            )));
        }
        Ok(Self {
            app_token: required("CORTEXFS_SLACK_APP_TOKEN")?,
            bot_token: required("CORTEXFS_SLACK_BOT_TOKEN")?,
            api_base: optional("CORTEXFS_SLACK_API_BASE", "https://slack.com/api"),
            socket,
            reconnect_seconds: number("CORTEXFS_SLACK_RECONNECT_SECONDS", 5)?.min(300),
            reply_timeout: Duration::from_secs(
                number("CORTEXFS_SLACK_REPLY_TIMEOUT_SECONDS", 30)?.clamp(1, 300),
            ),
        })
    }
}

fn required(name: &'static str) -> Result<String> {
    let value = env::var(name).map_err(|_error| Error::Config(format!("missing {name}")))?;
    (!value.is_empty())
        .then_some(value)
        .ok_or_else(|| Error::Config(format!("empty {name}")))
}

fn optional(name: &str, default: &str) -> String {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_owned())
}

fn number(name: &'static str, default: u64) -> Result<u64> {
    env::var(name)
        .ok()
        .filter(|value| !value.is_empty())
        .map_or(Ok(default), |value| {
            value
                .parse()
                .map_err(|error| Error::Config(format!("invalid {name}: {error}")))
        })
}
