use thiserror::Error;

use crate::{CHANNEL_SOCKET_ABI, ChannelError, ChannelFrame, ChannelFrameBody};

pub const MAX_CHANNEL_FRAME_BYTES: usize = 256 * 1024;

#[derive(Debug, Error)]
pub enum ChannelWireError {
    #[error("channel frame is too large")]
    FrameTooLarge,
    #[error("channel frame is not one newline-terminated JSON value")]
    InvalidFraming,
    #[error("channel frame has invalid field: {0}")]
    InvalidField(&'static str),
    #[error("channel frame JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl ChannelFrame {
    pub fn encode(&self) -> Result<Vec<u8>, ChannelWireError> {
        self.validate()
            .map_err(|_error| ChannelWireError::InvalidField("frame"))?;
        let mut bytes = serde_json::to_vec(self)?;
        if bytes.len().saturating_add(1) > MAX_CHANNEL_FRAME_BYTES {
            return Err(ChannelWireError::FrameTooLarge);
        }
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, ChannelWireError> {
        if bytes.len() < 2 || bytes.len() > MAX_CHANNEL_FRAME_BYTES || bytes.last() != Some(&b'\n')
        {
            return Err(ChannelWireError::InvalidFraming);
        }
        let body = bytes
            .get(..bytes.len() - 1)
            .ok_or(ChannelWireError::InvalidFraming)?;
        if body.contains(&b'\n') {
            return Err(ChannelWireError::InvalidFraming);
        }
        let frame: Self = serde_json::from_slice(body)?;
        frame
            .validate()
            .map_err(|_error| ChannelWireError::InvalidField("frame"))?;
        Ok(frame)
    }

    #[expect(
        clippy::pattern_type_mismatch,
        reason = "match ergonomics keep borrowed frame fields readable"
    )]
    pub fn validate(&self) -> Result<(), ChannelError> {
        if self.abi != CHANNEL_SOCKET_ABI {
            return Err(ChannelError::Protocol(format!(
                "unsupported channel socket ABI: {}",
                self.abi
            )));
        }
        match &self.frame {
            ChannelFrameBody::Inbound { event_id, message } => {
                valid(event_id)?;
                message.body.validate()
            }
            ChannelFrameBody::InboundEvent { event_id, event } => {
                valid(event_id)?;
                event.validate()
            }
            ChannelFrameBody::Deliver {
                request_id,
                message,
            }
            | ChannelFrameBody::Outbound {
                request_id,
                message,
            } => {
                valid(request_id)?;
                message.body.validate()
            }
            ChannelFrameBody::Effect {
                request_id, effect, ..
            } => {
                valid(request_id)?;
                effect.validate()
            }
            ChannelFrameBody::Command {
                request_id,
                session,
                command_id,
                ..
            }
            | ChannelFrameBody::CommandResult {
                request_id,
                session,
                command_id,
                ..
            } => {
                valid(request_id)?;
                valid(session)?;
                valid(command_id)
            }
            ChannelFrameBody::Error {
                request_id,
                code,
                message,
                ..
            } => {
                request_id.as_deref().map(valid).transpose()?;
                valid(code)?;
                valid(message)
            }
            ChannelFrameBody::Hello { request_id, .. }
            | ChannelFrameBody::Start { request_id }
            | ChannelFrameBody::Stop { request_id }
            | ChannelFrameBody::Receipt { request_id, .. }
            | ChannelFrameBody::HealthRequest { request_id }
            | ChannelFrameBody::HealthResponse { request_id, .. } => valid(request_id),
            ChannelFrameBody::Health { .. } | ChannelFrameBody::Event { .. } => Ok(()),
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
