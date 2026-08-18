use serde::{Deserialize, Serialize};

/// Features that an adapter can expose without platform-specific branching.
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent capability flags are the stable serialized wire shape"
)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChannelCapabilities {
    pub receive: bool,
    pub send: bool,
    pub group: bool,
    pub threads: bool,
    pub media: bool,
    /// Compatibility aggregate: either direction exposes attachments.
    pub attachments: bool,
    pub receive_attachments: bool,
    pub send_attachments: bool,
    pub audio: bool,
    pub video: bool,
    pub reactions: bool,
    pub typing: bool,
    pub streaming: bool,
    pub draft_updates: bool,
    pub multi_message_streaming: bool,
    /// The adapter can present and correlate runtime-initiated commands.
    pub commands: bool,
    /// The adapter can present a provider-neutral single-choice prompt.
    pub choices: bool,
    /// The adapter can collect more than one choice from one prompt.
    pub multi_choice: bool,
    pub polling: bool,
    pub long_polling: bool,
    pub websocket: bool,
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
            group: false,
            threads: false,
            media: false,
            attachments: false,
            receive_attachments: false,
            send_attachments: false,
            audio: false,
            video: false,
            reactions: false,
            typing: false,
            streaming: false,
            draft_updates: false,
            multi_message_streaming: false,
            commands: false,
            choices: false,
            multi_choice: false,
            polling: false,
            long_polling: false,
            websocket: false,
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
