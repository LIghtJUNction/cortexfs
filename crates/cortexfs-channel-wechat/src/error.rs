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
    #[error("channel driver failed: {0}")]
    Driver(#[from] cortexfs_channels::ChannelDriverError),
    #[error("channel message failed: {0}")]
    Channel(#[from] cortexfs_channels::ChannelError),
    #[error("HTTP operation failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("JSON operation failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("channel task failed: {0}")]
    Task(String),
    #[error("WeChat API rejected the request: {0}")]
    Api(String),
    #[error("WeChat protocol error: {0}")]
    Protocol(String),
}
