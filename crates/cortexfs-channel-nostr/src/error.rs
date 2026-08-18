#![expect(
    clippy::redundant_pub_crate,
    reason = "the typed error is shared by private driver modules"
)]

use std::fmt::Display;

use thiserror::Error;

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub(crate) enum Error {
    #[error("configuration error: {0}")]
    Config(String),
    #[error("channel driver failed: {0}")]
    Driver(#[from] cortexfs_channels::ChannelDriverError),
    #[error("channel protocol error: {0}")]
    Protocol(String),
    #[error("Nostr operation failed: {0}")]
    Nostr(String),
    #[error("worker task failed: {0}")]
    Task(String),
}

pub(crate) fn nostr(error: impl Display) -> Error {
    Error::Nostr(error.to_string())
}
