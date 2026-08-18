use serde::{Deserialize, Serialize};

use crate::{ChannelAction, ChannelError, MessageBody};

/// A platform-neutral live effect addressed by the channel runtime.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChannelEffect {
    Typing {
        active: bool,
    },
    Preview {
        text: String,
    },
    Reaction {
        message_id: String,
        emoji: String,
        remove: bool,
    },
    Edit {
        message_id: String,
        body: MessageBody,
    },
    Delete {
        message_id: String,
    },
    MarkRead {
        message_id: String,
    },
    Pin {
        message_id: String,
    },
    Unpin {
        message_id: String,
    },
    Redact {
        message_id: String,
        reason: Option<String>,
    },
}

impl ChannelEffect {
    /// Returns the capability needed to apply this live effect.
    #[must_use]
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "matching the borrowed effect keeps the action lookup allocation-free"
    )]
    pub const fn action(&self) -> ChannelAction {
        match self {
            Self::Typing { .. } => ChannelAction::Typing,
            Self::Preview { .. } => ChannelAction::Preview,
            Self::Reaction { .. } => ChannelAction::Reaction,
            Self::Edit { .. } => ChannelAction::Edit,
            Self::Delete { .. } => ChannelAction::Delete,
            Self::MarkRead { .. } => ChannelAction::MarkRead,
            Self::Pin { .. } => ChannelAction::Pin,
            Self::Unpin { .. } => ChannelAction::Unpin,
            Self::Redact { .. } => ChannelAction::Redact,
        }
    }

    #[expect(
        clippy::pattern_type_mismatch,
        reason = "match ergonomics keep borrowed effect fields readable"
    )]
    pub fn validate(&self) -> Result<(), ChannelError> {
        match self {
            Self::Typing { .. } => Ok(()),
            Self::Preview { text } => valid("preview", text),
            Self::Reaction {
                message_id, emoji, ..
            } => valid(message_id, emoji),
            Self::Edit { message_id, body } => {
                body.validate()?;
                valid(message_id, "ok")
            }
            Self::Delete { message_id }
            | Self::MarkRead { message_id }
            | Self::Pin { message_id }
            | Self::Unpin { message_id } => valid(message_id, "ok"),
            Self::Redact { message_id, reason } => {
                valid(message_id, "ok")?;
                if reason.as_deref().is_some_and(|value| value.contains('\0')) {
                    return Err(ChannelError::InvalidMessage(
                        "invalid channel effect".to_owned(),
                    ));
                }
                Ok(())
            }
        }
    }
}

fn valid(id: &str, value: &str) -> Result<(), ChannelError> {
    if id.is_empty() || id.contains('\0') || value.is_empty() {
        Err(ChannelError::InvalidMessage(
            "invalid channel effect".to_owned(),
        ))
    } else {
        Ok(())
    }
}
