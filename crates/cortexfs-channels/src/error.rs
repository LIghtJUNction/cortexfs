use std::time::Duration;

/// Stable failures returned by channel adapters and the registry.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ChannelError {
    #[error("invalid channel value: {0}")]
    InvalidValue(String),
    #[error("invalid channel message: {0}")]
    InvalidMessage(String),
    #[error("channel already registered: {0}")]
    DuplicateChannel(String),
    #[error("unknown channel: {0}")]
    UnknownChannel(String),
    #[error("channel operation unsupported: {0}")]
    Unsupported(String),
    #[error("channel authentication failed")]
    Authentication,
    #[error("channel sender is not authorized; configure allowed sender IDs")]
    SenderDenied,
    #[error("channel rate limited")]
    RateLimited { retry: RetryHint },
    #[error("channel transport failed: {0}")]
    Transport(String),
    #[error("channel protocol failed: {0}")]
    Protocol(String),
    #[error("channel is closed")]
    Closed,
}

/// Optional retry advice attached to a rate-limit failure.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RetryHint(Option<Duration>);

impl RetryHint {
    #[must_use]
    pub const fn none() -> Self {
        Self(None)
    }

    #[must_use]
    pub const fn after(delay: Duration) -> Self {
        Self(Some(delay))
    }

    #[must_use]
    pub const fn delay(self) -> Option<Duration> {
        self.0
    }
}
