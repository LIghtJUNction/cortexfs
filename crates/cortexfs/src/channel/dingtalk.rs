use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use reqwest::blocking::Client;

use super::bridge::AgentChannelBridge;

mod api;
mod config;
mod control;
mod handle;
mod parse;
mod transport;

pub use config::{DingTalkConfig, DingTalkError};

pub(super) type Webhooks = Arc<Mutex<HashMap<String, String>>>;

pub fn run(config: &DingTalkConfig, bridge: &AgentChannelBridge) -> Result<(), DingTalkError> {
    let client = Client::builder()
        .timeout(Duration::from_secs(35))
        .build()
        .map_err(DingTalkError::Http)?;
    let webhooks = Arc::new(Mutex::new(HashMap::new()));
    let control = control::start(config, bridge, &client, Arc::clone(&webhooks))?;
    loop {
        control
            .check()
            .map_err(|error| DingTalkError::Protocol(error.to_string()))?;
        match handle::run_once(config, bridge, &client, &webhooks) {
            Ok(()) => {}
            Err(DingTalkError::Config(message)) => return Err(DingTalkError::Config(message)),
            Err(_error) => {}
        }
        thread::sleep(Duration::from_secs(5));
    }
}
