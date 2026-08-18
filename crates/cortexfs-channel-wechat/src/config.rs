#![expect(
    clippy::field_scoped_visibility_modifiers,
    clippy::redundant_pub_crate,
    reason = "configuration is shared by private driver modules"
)]

use std::{collections::BTreeSet, env, path::PathBuf, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::error::{Error, Result};

pub(crate) struct Config {
    pub(crate) token: String,
    pub(crate) api_base: String,
    pub(crate) allowed_users: BTreeSet<String>,
    pub(crate) socket: PathBuf,
    pub(crate) poll_timeout: Duration,
    pub(crate) reply_timeout: Duration,
    pub(crate) channel_version: String,
    pub(crate) wechat_uin: String,
}

impl Config {
    pub(crate) fn load() -> Result<Self> {
        let expected = cortexfs_paths::channel_driver_socket("wechat");
        let socket = PathBuf::from(
            env::var("CORTEXFS_CHANNEL_SOCKET").unwrap_or_else(|_| expected.display().to_string()),
        );
        if socket != expected {
            return Err(Error::Config(format!(
                "CORTEXFS_CHANNEL_SOCKET must be {}",
                expected.display()
            )));
        }
        let api_base = env::var("CORTEXFS_WECHAT_API_BASE")
            .unwrap_or_else(|_| "https://ilinkai.weixin.qq.com".to_owned())
            .trim_end_matches('/')
            .to_owned();
        if !(api_base.starts_with("https://") || api_base.starts_with("http://")) {
            return Err(Error::Config(
                "CORTEXFS_WECHAT_API_BASE is invalid".to_owned(),
            ));
        }
        Ok(Self {
            token: required("CORTEXFS_WECHAT_TOKEN")?,
            api_base,
            allowed_users: list("CORTEXFS_WECHAT_ALLOWED_USERS"),
            socket,
            poll_timeout: seconds("CORTEXFS_WECHAT_POLL_TIMEOUT_SECONDS", 40)?,
            reply_timeout: seconds("CORTEXFS_WECHAT_REPLY_TIMEOUT_SECONDS", 600)?,
            channel_version: env::var("CORTEXFS_WECHAT_CHANNEL_VERSION")
                .unwrap_or_else(|_| "cortexfs/0.1".to_owned()),
            wechat_uin: unique_uin(),
        })
    }

    pub(crate) fn accepts(&self, user: &str) -> bool {
        self.allowed_users
            .iter()
            .any(|entry| entry == "*" || entry == user)
    }
}

fn required(name: &'static str) -> Result<String> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Error::Config(format!("{name} is required")))
}

fn list(name: &'static str) -> BTreeSet<String> {
    env::var(name)
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn seconds(name: &'static str, default: u64) -> Result<Duration> {
    let value = env::var(name).unwrap_or_else(|_| default.to_string());
    let seconds = value
        .parse::<u64>()
        .map_err(|_error| Error::Config(format!("{name} is invalid")))?
        .clamp(1, 3600);
    Ok(Duration::from_secs(seconds))
}

fn unique_uin() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |value| value.as_nanos());
    STANDARD.encode(format!("cortexfs:{}:{}", std::process::id(), now))
}
