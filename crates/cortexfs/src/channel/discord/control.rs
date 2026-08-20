use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

use cortexfs_channels::{ChannelActions, ChannelCapabilities, ChannelDriverSession, ChannelId};
use reqwest::blocking::Client;

use super::{DiscordConfig, DiscordError};
use crate::channel::{
    bridge::AgentChannelBridge,
    driver::{self, DriverConfig, DriverHub},
};

mod serve;

pub(super) struct Control {
    errors: Receiver<DiscordError>,
}

impl Control {
    pub(super) fn check(&self) -> Result<(), DiscordError> {
        match self.errors.try_recv() {
            Ok(error) => Err(error),
            Err(TryRecvError::Empty) => Ok(()),
            Err(TryRecvError::Disconnected) => {
                Err(DiscordError::Protocol("Discord control stopped".to_owned()))
            }
        }
    }
}

pub(super) fn start(
    config: &DiscordConfig,
    bridge: &AgentChannelBridge,
    client: &Client,
) -> Result<Control, DiscordError> {
    let channel = config
        .channel
        .clone()
        .unwrap_or_else(|| ChannelId::from_static("discord"));
    let socket = cortexfs_paths::channel_driver_socket(channel.as_str());
    let (errors, receiver) = mpsc::sync_channel(2);
    let runtime = DriverConfig {
        socket: socket.clone(),
        channel: channel.clone(),
        bridge: bridge.clone(),
        hub: DriverHub::default(),
    };
    let runtime_errors = errors.clone();
    std::thread::spawn(move || {
        if let Err(error) = driver::run(&runtime) {
            let _ignored = runtime_errors.send(DiscordError::Runtime(error));
        }
    });
    let session = ChannelDriverSession::connect_retry(
        &socket,
        &channel,
        ChannelCapabilities {
            tool_control: true,
            websocket: true,
            ..ChannelCapabilities::empty()
        },
        ChannelActions::empty(),
        "discord-control",
        Duration::from_secs(5),
    )?;
    let worker_config = config.clone();
    let worker_client = client.clone();
    std::thread::spawn(move || {
        if let Err(error) = serve::run(&session, &worker_client, &worker_config) {
            let _ignored = errors.send(error);
        }
    });
    Ok(Control { errors: receiver })
}
