#![expect(
    clippy::field_scoped_visibility_modifiers,
    clippy::redundant_pub_crate,
    reason = "configuration is shared by private driver modules"
)]

use std::{env, path::PathBuf, time::Duration};

use crate::error::{Error, Result};

#[derive(Clone)]
pub(crate) struct Config {
    pub(crate) bot_id: String,
    pub(crate) secret: String,
    pub(crate) allowed_users: Vec<String>,
    pub(crate) allowed_groups: Vec<String>,
    pub(crate) socket: PathBuf,
    pub(crate) reply_timeout: Duration,
}

impl Config {
    pub(crate) fn load() -> Result<Self> {
        let expected = cortexfs_paths::channel_driver_socket("wecom-ws");
        let socket = PathBuf::from(
            env::var("CORTEXFS_CHANNEL_SOCKET").unwrap_or_else(|_| expected.display().to_string()),
        );
        if socket != expected {
            return Err(Error::Config(format!(
                "CORTEXFS_CHANNEL_SOCKET must be {}",
                expected.display()
            )));
        }
        let seconds = env::var("CORTEXFS_WECOM_REPLY_TIMEOUT_SECONDS")
            .ok()
            .map(|value| value.parse::<u64>())
            .transpose()
            .map_err(|_error| Error::Config("reply timeout is invalid".to_owned()))?
            .unwrap_or(600)
            .clamp(1, 3600);
        Ok(Self {
            bot_id: required("CORTEXFS_WECOM_BOT_ID")?,
            secret: required("CORTEXFS_WECOM_SECRET")?,
            allowed_users: optional_list("CORTEXFS_WECOM_ALLOWED_USERS"),
            allowed_groups: optional_list("CORTEXFS_WECOM_ALLOWED_GROUPS"),
            socket,
            reply_timeout: Duration::from_secs(seconds),
        })
    }

    pub(crate) fn allowed(&self, user: &str, group: Option<&str>) -> bool {
        self.allowed_users
            .iter()
            .any(|entry| entry == "*" || entry == user)
            || group.is_some_and(|value| {
                self.allowed_groups
                    .iter()
                    .any(|entry| entry == "*" || entry == value)
            })
    }
}

fn required(name: &'static str) -> Result<String> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Error::Config(format!("{name} is required")))
}

fn optional_list(name: &'static str) -> Vec<String> {
    env::var(name)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}
