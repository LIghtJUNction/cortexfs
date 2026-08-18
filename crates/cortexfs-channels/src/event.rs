use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{ChannelError, MessageBody, MessageTarget, Participant};

/// Shared context carried by an incoming non-message channel event.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ChannelEventContext {
    pub target: MessageTarget,
    pub participant: Option<Participant>,
    pub timestamp_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

/// Platform-neutral changes and presence signals received from a channel.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChannelIncomingEvent {
    Reaction {
        context: ChannelEventContext,
        message_id: String,
        emoji: String,
        added: bool,
    },
    Typing {
        context: ChannelEventContext,
        active: bool,
    },
    MessageEdited {
        context: ChannelEventContext,
        message_id: String,
        body: MessageBody,
    },
    MessageDeleted {
        context: ChannelEventContext,
        message_id: String,
    },
    Read {
        context: ChannelEventContext,
        message_id: String,
    },
}

impl ChannelEventContext {
    pub fn validate(&self) -> Result<(), ChannelError> {
        valid(self.target.channel.as_str())?;
        valid(self.target.conversation.as_str())?;
        self.target.thread.as_deref().map(valid).transpose()?;
        self.target.reply_to.as_deref().map(valid).transpose()?;
        if let Some(participant) = self.participant.as_ref() {
            valid(&participant.id)?;
        }
        for (key, value) in &self.metadata {
            valid(key)?;
            valid(value)?;
        }
        Ok(())
    }
}

impl ChannelIncomingEvent {
    /// Rebinds an event to the configured channel instance.
    #[must_use]
    pub fn with_channel(mut self, channel: crate::ChannelId) -> Self {
        self.context_mut().target.channel = channel;
        self
    }

    #[must_use]
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "matching borrowed event variants keeps message ids zero-copy"
    )]
    pub fn message_id(&self) -> Option<&str> {
        match self {
            Self::Reaction { message_id, .. }
            | Self::MessageEdited { message_id, .. }
            | Self::MessageDeleted { message_id, .. }
            | Self::Read { message_id, .. } => Some(message_id),
            Self::Typing { .. } => None,
        }
    }

    #[must_use]
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "matching borrowed event variants keeps context access zero-copy"
    )]
    pub fn context(&self) -> &ChannelEventContext {
        match self {
            Self::Reaction { context, .. }
            | Self::Typing { context, .. }
            | Self::MessageEdited { context, .. }
            | Self::MessageDeleted { context, .. }
            | Self::Read { context, .. } => context,
        }
    }

    #[must_use]
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "matching borrowed event variants keeps context mutation allocation-free"
    )]
    pub fn context_mut(&mut self) -> &mut ChannelEventContext {
        match self {
            Self::Reaction { context, .. }
            | Self::Typing { context, .. }
            | Self::MessageEdited { context, .. }
            | Self::MessageDeleted { context, .. }
            | Self::Read { context, .. } => context,
        }
    }

    #[expect(
        clippy::pattern_type_mismatch,
        reason = "validation only borrows event fields"
    )]
    pub fn validate(&self) -> Result<(), ChannelError> {
        self.context().validate()?;
        match self {
            Self::Reaction {
                message_id, emoji, ..
            } => {
                valid(message_id)?;
                valid(emoji)
            }
            Self::Typing { .. } => Ok(()),
            Self::MessageEdited {
                message_id, body, ..
            } => {
                valid(message_id)?;
                body.validate()
            }
            Self::MessageDeleted { message_id, .. } | Self::Read { message_id, .. } => {
                valid(message_id)
            }
        }
    }
}

fn valid(value: &str) -> Result<(), ChannelError> {
    if value.is_empty() || value.len() > 256 || value.contains('\0') {
        Err(ChannelError::InvalidValue(value.to_owned()))
    } else {
        Ok(())
    }
}
