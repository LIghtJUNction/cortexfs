use thiserror::Error;

use crate::{frame::ModuleFrame, valid_name};

/// Versioned JSONL-over-Unix-socket contract for external modules.
pub const CORTEX_MODULE_WIRE_ABI: &str = "cortexfs.module.socket/v1";
/// Maximum encoded frame size, including its trailing newline.
pub const MAX_MODULE_FRAME_BYTES: usize = 1024 * 1024;

/// Error while validating or framing an external-module message.
#[derive(Debug, Error)]
pub enum ModuleWireError {
    #[error("module frame is too large")]
    FrameTooLarge,
    #[error("module frame must contain exactly one newline-terminated JSON value")]
    InvalidFraming,
    #[error("invalid module frame field: {0}")]
    InvalidField(&'static str),
    #[error("module frame JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl ModuleFrame {
    /// Encodes one validated JSONL frame for a Unix socket.
    pub fn encode(&self) -> Result<Vec<u8>, ModuleWireError> {
        self.validate()?;
        let mut bytes = serde_json::to_vec(self)?;
        if bytes.len().saturating_add(1) > MAX_MODULE_FRAME_BYTES {
            return Err(ModuleWireError::FrameTooLarge);
        }
        bytes.push(b'\n');
        Ok(bytes)
    }

    /// Decodes one complete JSONL frame and rejects concatenated frames.
    pub fn decode(bytes: &[u8]) -> Result<Self, ModuleWireError> {
        if bytes.len() < 2 || bytes.len() > MAX_MODULE_FRAME_BYTES || bytes.last() != Some(&b'\n') {
            return Err(ModuleWireError::InvalidFraming);
        }
        let body = bytes
            .get(..bytes.len() - 1)
            .ok_or(ModuleWireError::InvalidFraming)?;
        if body.contains(&b'\n') {
            return Err(ModuleWireError::InvalidFraming);
        }
        let frame: Self = serde_json::from_slice(body)?;
        frame.validate()?;
        Ok(frame)
    }

    /// Validates version, identifiers, and metadata without performing I/O.
    pub fn validate(&self) -> Result<(), ModuleWireError> {
        match *self {
            Self::Hello {
                ref abi,
                ref instance,
            } => {
                if abi != CORTEX_MODULE_WIRE_ABI {
                    return Err(ModuleWireError::InvalidField("abi"));
                }
                valid(instance, "instance")
            }
            Self::Ready { ref metadata } => metadata
                .is_valid()
                .then_some(())
                .ok_or(ModuleWireError::InvalidField("metadata")),
            Self::Lifecycle { ref request_id, .. } | Self::Result { ref request_id, .. } => {
                valid(request_id, "request_id")
            }
            Self::Call {
                ref request_id,
                ref method,
                ..
            } => {
                valid(request_id, "request_id")?;
                valid(method, "method")
            }
            Self::Event { ref name, .. } => valid(name, "name"),
            Self::Error {
                ref request_id,
                ref code,
                ref message,
            } => {
                request_id
                    .as_deref()
                    .map_or(Ok(()), |value| valid(value, "request_id"))?;
                valid(code, "code")?;
                if message.is_empty() || message.contains('\0') {
                    return Err(ModuleWireError::InvalidField("message"));
                }
                Ok(())
            }
        }
    }
}

fn valid(value: &str, field: &'static str) -> Result<(), ModuleWireError> {
    valid_name(value)
        .then_some(())
        .ok_or(ModuleWireError::InvalidField(field))
}
