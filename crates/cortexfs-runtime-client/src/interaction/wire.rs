use super::{InteractionFrame, InteractionV2Frame};
use thiserror::Error;

/// Maximum one-line interaction frame, including its newline.
pub const MAX_INTERACTION_FRAME_BYTES: usize = 256 * 1024;

#[derive(Debug, Error)]
pub enum InteractionWireError {
    #[error("interaction frame is too large")]
    FrameTooLarge,
    #[error("interaction frame must be one newline-terminated JSON value")]
    InvalidFraming,
    #[error("interaction frame has invalid field: {0}")]
    InvalidField(&'static str),
    #[error("interaction frame JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

macro_rules! wire {
    ($frame:ty) => {
        impl $frame {
            pub fn encode(&self) -> Result<Vec<u8>, InteractionWireError> {
                self.validate()
                    .map_err(|_error| InteractionWireError::InvalidField("frame"))?;
                let mut bytes = serde_json::to_vec(self)?;
                if bytes.len().saturating_add(1) > MAX_INTERACTION_FRAME_BYTES {
                    return Err(InteractionWireError::FrameTooLarge);
                }
                bytes.push(b'\n');
                Ok(bytes)
            }

            pub fn decode(bytes: &[u8]) -> Result<Self, InteractionWireError> {
                if bytes.len() < 2
                    || bytes.len() > MAX_INTERACTION_FRAME_BYTES
                    || bytes.last() != Some(&b'\n')
                {
                    return Err(InteractionWireError::InvalidFraming);
                }
                let body = bytes
                    .get(..bytes.len() - 1)
                    .ok_or(InteractionWireError::InvalidFraming)?;
                if body.contains(&b'\n') {
                    return Err(InteractionWireError::InvalidFraming);
                }
                let frame: Self = serde_json::from_slice(body)?;
                frame
                    .validate()
                    .map_err(|_error| InteractionWireError::InvalidField("frame"))?;
                Ok(frame)
            }
        }
    };
}

wire!(InteractionFrame);
wire!(InteractionV2Frame);
