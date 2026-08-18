#![expect(
    clippy::redundant_pub_crate,
    reason = "typed errors are shared by private driver modules"
)]

use thiserror::Error;

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("channel driver failed: {0}")]
    Driver(#[from] cortexfs_channels::ChannelDriverError),
    #[error("channel message failed: {0}")]
    Channel(#[from] cortexfs_channels::ChannelError),
    #[error("HTTP operation failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON operation failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("URL decoding failed: {0}")]
    Url(#[from] url::ParseError),
    #[error("I/O operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("channel task failed: {0}")]
    Task(String),
    #[error("voice protocol error: {0}")]
    Protocol(String),
}
