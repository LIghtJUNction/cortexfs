use serde::{Deserialize, Serialize};

use crate::{ChannelCommand, ChannelEffect, ChannelError, MessageTarget, OutboundMessage};

/// A provider-neutral request from a runtime tool to a channel driver.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChannelControlAction {
    Send {
        message: OutboundMessage,
    },
    Effect {
        target: MessageTarget,
        effect: ChannelEffect,
    },
    Command {
        session: String,
        command_id: String,
        command: ChannelCommand,
        target: Option<MessageTarget>,
    },
}

impl ChannelControlAction {
    #[expect(
        clippy::pattern_type_mismatch,
        reason = "matching borrowed control actions keeps validation allocation-free"
    )]
    pub fn validate(&self) -> Result<(), ChannelError> {
        match self {
            Self::Send { message } => {
                valid(message.target.channel.as_str())?;
                valid(message.target.conversation.as_str())?;
                message.body.validate()
            }
            Self::Effect { target, effect } => {
                valid(target.conversation.as_str())?;
                effect.validate()
            }
            Self::Command {
                session,
                command_id,
                command,
                target,
            } => {
                valid(session)?;
                valid(command_id)?;
                if let Some(target) = target {
                    valid(target.conversation.as_str())?;
                }
                serde_json::to_value(command).map(|_| ()).map_err(|error| {
                    ChannelError::Protocol(format!("invalid channel command: {error}"))
                })
            }
        }
    }
}

fn valid(value: &str) -> Result<(), ChannelError> {
    if value.is_empty() || value.contains('\0') {
        Err(ChannelError::InvalidValue(value.to_owned()))
    } else {
        Ok(())
    }
}
