#![expect(
    clippy::redundant_pub_crate,
    reason = "error types are shared only by private driver modules"
)]

pub(crate) type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    #[error("invalid Slack configuration: {0}")]
    Config(String),
    #[error("channel driver failed: {0}")]
    Driver(#[from] cortexfs_channels::ChannelDriverError),
    #[error("channel protocol failed: {0}")]
    Channel(#[from] cortexfs_channels::ChannelError),
    #[error("Slack HTTP request failed")]
    Http(#[source] reqwest::Error),
    #[error("Slack returned an invalid response: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Slack API rejected the request: {0}")]
    Api(String),
    #[error("Slack WebSocket failed: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    #[error("background task failed: {0}")]
    Task(String),
}
