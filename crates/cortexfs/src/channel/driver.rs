use std::{
    io::Write,
    net::Shutdown,
    os::unix::net::{UnixListener, UnixStream},
    path::PathBuf,
    sync::mpsc::TrySendError,
};

use cortexfs_channels::{ChannelFrame, ChannelId, ChannelWireError};

use super::{bridge::AgentChannelBridge, driverprogress};

mod hub;
mod pool;
mod reader;
mod session;
mod worker;

pub use hub::DriverHub;

#[derive(Clone, Debug)]
pub struct DriverConfig {
    pub socket: PathBuf,
    pub channel: ChannelId,
    pub bridge: AgentChannelBridge,
    pub hub: DriverHub,
}

pub fn run(config: &DriverConfig) -> Result<(), DriverError> {
    let listener = UnixListener::bind(&config.socket).map_err(DriverError::Io)?;
    let sessions = pool::spawn(config);
    loop {
        let (stream, _) = listener.accept().map_err(DriverError::Io)?;
        match sessions.try_send(stream) {
            Ok(()) => {}
            Err(TrySendError::Full(stream)) => {
                let _ignored = stream.shutdown(Shutdown::Both);
            }
            Err(TrySendError::Disconnected(_stream)) => return Err(DriverError::SessionPool),
        }
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
    #[error("channel driver command timed out")]
    CommandTimeout,
    #[error("channel driver session pool stopped")]
    SessionPool,
    #[error("channel driver lock is poisoned")]
    Lock,
    #[error("channel driver capability rejected the request")]
    Rejected,
}

pub(super) fn write(stream: &mut UnixStream, frame: &ChannelFrame) -> Result<(), DriverError> {
    stream.write_all(&frame.encode().map_err(DriverError::Frame)?)?;
    stream.flush().map_err(DriverError::Io)
}
