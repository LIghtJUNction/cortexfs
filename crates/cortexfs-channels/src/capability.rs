use serde::{Deserialize, Serialize};

/// Features that an adapter can expose without platform-specific branching.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent capability flags are the stable serialized wire shape"
)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelCapabilities {
    pub receive: bool,
    pub send: bool,
    pub threads: bool,
    pub media: bool,
    pub reactions: bool,
    pub typing: bool,
    pub webhook: bool,
}

impl ChannelCapabilities {
    #[must_use]
    pub const fn text() -> Self {
        Self {
            receive: true,
            send: true,
            ..Self::empty()
        }
    }

    #[must_use]
    pub const fn empty() -> Self {
        Self {
            receive: false,
            send: false,
            threads: false,
            media: false,
            reactions: false,
            typing: false,
            webhook: false,
        }
    }
}

/// Liveness of one adapter, separate from capability discovery.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum HealthState {
    Ready,
    Degraded,
    Unavailable,
}

/// Health result that can be surfaced without leaking credentials.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelHealth {
    pub state: HealthState,
    pub detail: Option<String>,
}

impl ChannelHealth {
    #[must_use]
    pub const fn ready() -> Self {
        Self {
            state: HealthState::Ready,
            detail: None,
        }
    }
}
