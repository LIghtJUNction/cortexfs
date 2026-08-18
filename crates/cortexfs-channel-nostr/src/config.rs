#![expect(
    clippy::field_scoped_visibility_modifiers,
    clippy::redundant_pub_crate,
    reason = "configuration is shared by private driver modules"
)]

use std::{collections::BTreeSet, env, path::PathBuf};

use nostr_sdk::prelude::{Keys, PublicKey};

use crate::error::{Error, Result};

pub(crate) struct Config {
    pub(crate) keys: Keys,
    pub(crate) relays: Vec<String>,
    pub(crate) allowed: BTreeSet<PublicKey>,
    pub(crate) socket: PathBuf,
    pub(crate) reply_timeout: std::time::Duration,
}

impl Config {
    pub(crate) fn load() -> Result<Self> {
        let key = required("CORTEXFS_NOSTR_PRIVATE_KEY")?;
        let keys = Keys::parse(&key)
            .map_err(|_error| Error::Config("CORTEXFS_NOSTR_PRIVATE_KEY is invalid".to_owned()))?;
        let relays = list("CORTEXFS_NOSTR_RELAYS")?;
        let allowed = parse_allowed(env::var("CORTEXFS_NOSTR_ALLOWED_USERS").ok())?;
        let expected = cortexfs_paths::channel_driver_socket("nostr");
        let socket = PathBuf::from(
            env::var("CORTEXFS_CHANNEL_SOCKET").unwrap_or_else(|_| expected.display().to_string()),
        );
        if socket != expected {
            return Err(Error::Config(format!(
                "CORTEXFS_CHANNEL_SOCKET must be {}",
                expected.display()
            )));
        }
        let seconds = env::var("CORTEXFS_NOSTR_REPLY_TIMEOUT_SECONDS")
            .ok()
            .map(|value| value.parse::<u64>())
            .transpose()
            .map_err(|_error| {
                Error::Config("CORTEXFS_NOSTR_REPLY_TIMEOUT_SECONDS is invalid".to_owned())
            })?
            .unwrap_or(600)
            .clamp(1, 3600);
        Ok(Self {
            keys,
            relays,
            allowed,
            socket,
            reply_timeout: std::time::Duration::from_secs(seconds),
        })
    }

    pub(crate) fn accepts(&self, key: &PublicKey) -> bool {
        self.allowed.contains(key)
    }
}

fn required(name: &'static str) -> Result<String> {
    env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| Error::Config(format!("{name} is required")))
}

fn list(name: &'static str) -> Result<Vec<String>> {
    let values = required(name)?
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Err(Error::Config(format!("{name} is empty")));
    }
    Ok(values)
}

fn parse_allowed(value: Option<String>) -> Result<BTreeSet<PublicKey>> {
    value
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| {
            PublicKey::parse(item).map_err(|_error| {
                Error::Config("CORTEXFS_NOSTR_ALLOWED_USERS is invalid".to_owned())
            })
        })
        .collect()
}
