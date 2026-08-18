#![expect(
    clippy::redundant_pub_crate,
    reason = "typed process errors are shared by private driver modules"
)]

use cortexfs_channels::{ChannelDriverError, ChannelError};
use thiserror::Error;

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("MQTT configuration failed: {0}")]
    Config(String),
    #[error("MQTT client failed: {0}")]
    Client(#[from] rumqttc::ClientError),
    #[error("MQTT connection failed: {0}")]
    Connection(#[source] Box<rumqttc::ConnectionError>),
    #[error("channel driver failed: {0}")]
    Driver(#[from] ChannelDriverError),
    #[error("channel message failed: {0}")]
    Channel(#[from] ChannelError),
    #[error("MQTT task failed: {0}")]
    Task(String),
    #[error("MQTT connection closed")]
    Closed,
}

impl From<rumqttc::ConnectionError> for Error {
    fn from(error: rumqttc::ConnectionError) -> Self {
        Self::Connection(Box::new(error))
    }
}
