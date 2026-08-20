#![expect(
    clippy::redundant_pub_crate,
    reason = "the control service is shared by private channel hosts"
)]

use std::{sync::mpsc, time::Duration};

use cortexfs_channels::{
    ChannelCommand, ChannelCommandResult, ChannelDriverError, ChannelDriverSession, ChannelEffect,
    MessageTarget, OutboundMessage,
};

use super::driver::{self, DriverConfig, DriverError, DriverHub};

mod codec;
mod config;
mod serve;

pub use codec::{CodecHandler, CodecTransport};
pub use config::ChannelControlConfig;

#[derive(Debug, thiserror::Error)]
pub enum ChannelControlError {
    #[error("channel control driver failed: {0}")]
    Driver(#[from] DriverError),
    #[error("channel control connection failed: {0}")]
    Connection(#[from] ChannelDriverError),
    #[error("channel control operation failed: {0}")]
    Operation(String),
    #[error("channel control stopped")]
    Stopped,
}

pub trait ChannelControlHandler: Send {
    fn outbound(&mut self, message: &OutboundMessage) -> Result<(), ChannelControlError>;
    fn effect(
        &mut self,
        target: &MessageTarget,
        effect: &ChannelEffect,
    ) -> Result<(), ChannelControlError>;
    fn command(
        &mut self,
        session: &str,
        command_id: &str,
        command: &ChannelCommand,
        target: Option<&MessageTarget>,
    ) -> Result<ChannelCommandResult, ChannelControlError>;
}

#[derive(Debug)]
pub struct ChannelControl {
    errors: mpsc::Receiver<ChannelControlError>,
}

impl ChannelControl {
    pub fn check(&self) -> Result<(), ChannelControlError> {
        match self.errors.try_recv() {
            Ok(error) => Err(error),
            Err(mpsc::TryRecvError::Empty) => Ok(()),
            Err(mpsc::TryRecvError::Disconnected) => Err(ChannelControlError::Stopped),
        }
    }
}

pub fn start(config: ChannelControlConfig) -> Result<ChannelControl, ChannelControlError> {
    let (sender, errors) = mpsc::sync_channel(2);
    let runtime = DriverConfig {
        socket: config.socket.clone(),
        channel: config.channel.clone(),
        bridge: config.bridge,
        hub: DriverHub::default(),
    };
    let runtime_sender = sender.clone();
    std::thread::spawn(move || {
        if let Err(error) = driver::run(&runtime) {
            let _ignored = runtime_sender.send(error.into());
        }
    });
    let session = ChannelDriverSession::connect_retry(
        &config.socket,
        &config.channel,
        config.capabilities,
        config.actions,
        "channel-control",
        Duration::from_secs(5),
    )?;
    std::thread::spawn(move || {
        if let Err(error) = serve::run(session, config.handler) {
            let _ignored = sender.send(error);
        }
    });
    Ok(ChannelControl { errors })
}
