#![expect(
    clippy::redundant_pub_crate,
    reason = "typed process errors are shared by private driver modules"
)]

use thiserror::Error;

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("AMQP consumer closed")]
    Closed,
    #[error("AMQP message failed: {0}")]
    Message(String),
    #[error("AMQP operation failed: {0}")]
    Amqp(#[from] lapin::Error),
    #[error("channel driver failed: {0}")]
    Driver(#[from] cortexfs_channels::ChannelDriverError),
    #[error("channel message failed: {0}")]
    Channel(#[from] cortexfs_channels::ChannelError),
    #[error("worker task failed: {0}")]
    Task(String),
}
