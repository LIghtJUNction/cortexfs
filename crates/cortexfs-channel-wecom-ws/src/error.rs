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
    #[error("WebSocket operation failed: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("JSON frame failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("channel task failed: {0}")]
    Task(String),
    #[error("protocol error: {0}")]
    Protocol(String),
}
