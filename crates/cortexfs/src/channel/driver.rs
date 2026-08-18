use std::{
    io::Write,
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
};

use cortexfs_channels::{ChannelFrame, ChannelId, ChannelWireError};

use super::{bridge::AgentChannelBridge, driverprogress};

mod hub;
mod reader;
mod session;
mod worker;

pub use hub::DriverHub;

#[derive(Debug)]
pub struct DriverConfig {
    pub socket: PathBuf,
    pub channel: ChannelId,
    pub bridge: AgentChannelBridge,
    pub hub: DriverHub,
}

pub fn run(config: &DriverConfig) -> Result<(), DriverError> {
    let listener = UnixListener::bind(&config.socket).map_err(DriverError::Io)?;
    loop {
        let (stream, _) = listener.accept().map_err(DriverError::Io)?;
        session::serve(stream, config)?;
    }
}

#[cfg(test)]
pub(super) fn serve_once(stream: UnixStream, config: &DriverConfig) -> Result<(), DriverError> {
    session::serve(stream, config)
}

#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("channel driver I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("channel driver frame failed: {0}")]
    Frame(#[source] ChannelWireError),
    #[error("channel driver is not connected")]
    Unavailable,
    #[error("channel driver receipt timed out")]
    ReceiptTimeout,
    #[error("channel driver lock is poisoned")]
    Lock,
}

pub(super) fn write(stream: &mut UnixStream, frame: &ChannelFrame) -> Result<(), DriverError> {
    stream.write_all(&frame.encode().map_err(DriverError::Frame)?)?;
    stream.flush().map_err(DriverError::Io)
}

pub(super) fn new_broker() -> driverprogress::CommandBroker {
    driverprogress::new_broker()
}
