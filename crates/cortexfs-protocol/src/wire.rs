use std::fmt;

/// External wire protocol understood by the `CortexFS` bridge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WireProtocol {
    OpenAiChat,
    OpenAiResponses,
    Gemini,
    Anthropic,
}

impl WireProtocol {
    /// Stable human-readable protocol name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiChat => "openai.chat",
            Self::OpenAiResponses => "openai.responses",
            Self::Gemini => "gemini",
            Self::Anthropic => "anthropic.messages",
        }
    }
}

impl fmt::Display for WireProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Conversion failure at an external protocol boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversionError {
    InvalidJson {
        protocol: WireProtocol,
        detail: String,
    },
    MissingField {
        protocol: WireProtocol,
        field: String,
    },
    InvalidField {
        protocol: WireProtocol,
        field: String,
    },
    UnsupportedField {
        protocol: WireProtocol,
        field: String,
    },
    Core(ProtocolError),
}

impl fmt::Display for ConversionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::InvalidJson {
                protocol,
                ref detail,
            } => {
                write!(f, "{protocol} JSON is invalid: {detail}")
            }
            Self::MissingField {
                protocol,
                ref field,
            } => {
                write!(f, "{protocol} field is missing: {field}")
            }
            Self::InvalidField {
                protocol,
                ref field,
            } => {
                write!(f, "{protocol} field is invalid: {field}")
            }
            Self::UnsupportedField {
                protocol,
                ref field,
            } => {
                write!(f, "{protocol} field is unsupported: {field}")
            }
            Self::Core(ref error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ConversionError {}

impl From<ProtocolError> for ConversionError {
    fn from(error: ProtocolError) -> Self {
        Self::Core(error)
    }
}

use crate::ProtocolError;
