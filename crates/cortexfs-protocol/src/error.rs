use serde::{Deserialize, Serialize};
use std::fmt;

/// Stable validation failure for the protocol IR.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProtocolError {
    UnsupportedProtocol(String),
    EmptyModel,
    EmptyMessages,
    EmptyRole,
    EmptyToolName,
    DuplicateTool(String),
    InvalidToolSchema(String),
    InvalidContext(String),
}

impl fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::UnsupportedProtocol(ref value) => write!(f, "unsupported protocol: {value}"),
            Self::EmptyModel => f.write_str("model name is empty"),
            Self::EmptyMessages => f.write_str("model request has no messages"),
            Self::EmptyRole => f.write_str("model message role is empty"),
            Self::EmptyToolName => f.write_str("tool name is empty"),
            Self::DuplicateTool(ref name) => write!(f, "duplicate tool: {name}"),
            Self::InvalidToolSchema(ref name) => write!(f, "invalid tool schema: {name}"),
            Self::InvalidContext(ref detail) => write!(f, "invalid context state: {detail}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

/// Provider failure normalized at the adapter boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderError {
    pub code: String,
    pub message: String,
    pub retryable: bool,
}

impl ProviderError {
    /// Creates a normalized provider failure.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>, retryable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            retryable,
        }
    }
}
