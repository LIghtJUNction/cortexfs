use std::{thread, time::Duration};

use reqwest::blocking::Client;

use super::bridge::AgentChannelBridge;

mod api;
mod config;
mod handle;
mod parse;
mod transport;

pub use config::{DingTalkConfig, DingTalkError};

pub fn run(config: &DingTalkConfig, bridge: &AgentChannelBridge) -> Result<(), DingTalkError> {
    let client = Client::builder()
        .timeout(Duration::from_secs(35))
        .build()
        .map_err(DingTalkError::Http)?;
    loop {
        match handle::run_once(config, bridge, &client) {
            Ok(()) => {}
            Err(DingTalkError::Config(message)) => return Err(DingTalkError::Config(message)),
            Err(_error) => {}
        }
        thread::sleep(Duration::from_secs(5));
    }
}
