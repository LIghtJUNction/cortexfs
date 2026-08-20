use cortexfs_channels::{ChannelDriverError, ChannelError};

/// Stable failures raised by the channel adapter host.
#[derive(Debug, thiserror::Error)]
pub enum ChannelSdkError {
    #[error("channel driver failed: {0}")]
    Driver(#[from] ChannelDriverError),
    #[error("channel adapter {operation} failed: {source}")]
    Adapter {
        operation: &'static str,
        source: ChannelError,
    },
}

impl ChannelSdkError {
    pub(crate) const fn adapter(operation: &'static str, source: ChannelError) -> Self {
        Self::Adapter { operation, source }
    }
}
