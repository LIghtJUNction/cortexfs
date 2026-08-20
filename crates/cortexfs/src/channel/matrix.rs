use std::{thread, time::Duration};

use reqwest::blocking::Client;

use super::bridge::AgentChannelBridge;

mod api;
mod config;
mod control;
mod sync;

pub use config::{MatrixConfig, MatrixError};

/// Runs a Matrix Client-Server `/sync` foreground loop.
pub fn run(config: &MatrixConfig, bridge: &AgentChannelBridge) -> Result<(), MatrixError> {
    let client = Client::builder()
        .timeout(Duration::from_secs(config.sync_seconds.saturating_add(20)))
        .build()
        .map_err(MatrixError::Http)?;
    let user_id = api::whoami(&client, config)?;
    let control = control::start(config, bridge, &client)?;
    let mut since = None;
    let mut transaction = 0_u64;
    loop {
        control
            .check()
            .map_err(|error| MatrixError::Protocol(error.to_string()))?;
        match sync::run_once(
            &client,
            config,
            bridge,
            &user_id,
            &mut since,
            &mut transaction,
        ) {
            Ok(()) => {}
            Err(_error) => thread::sleep(Duration::from_secs(5)),
        }
    }
}
