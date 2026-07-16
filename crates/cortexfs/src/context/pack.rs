pub use crate::context::inspect::inspect_context_pack_json;
pub use crate::context::source::{ContextPackSourceError, validate_context_pack_source};

/// Context pack validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextPackIssue {
    /// `pack.json` is not valid JSON.
    InvalidJson,
    /// Root JSON value does not contain an `items` array.
    ItemsNotArray,
    /// Pack item is not a JSON object.
    ItemNotObject(usize),
    /// Pack item does not identify an inspectable source.
    MissingSource(usize),
    /// Pack item `source` is present but is not a string.
    SourceNotString(usize),
    /// Pack item source is outside the allowed session-relative source set.
    InvalidSource {
        /// Zero-based item index.
        item: usize,
        /// Source value from the pack.
        source: String,
        /// Stable reason for refusal.
        reason: ContextPackSourceError,
    },
}

impl ContextPackIssue {
    /// Returns a stable short description of the issue kind.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match *self {
            Self::InvalidJson => "invalid json",
            Self::ItemsNotArray => "items not array",
            Self::ItemNotObject(_) => "item not object",
            Self::MissingSource(_) => "missing source",
            Self::SourceNotString(_) => "source not string",
            Self::InvalidSource { .. } => "invalid source",
        }
    }

    /// Returns the pack item index associated with this issue, when any.
    #[must_use]
    pub const fn item(&self) -> Option<usize> {
        match *self {
            Self::ItemNotObject(index)
            | Self::MissingSource(index)
            | Self::SourceNotString(index)
            | Self::InvalidSource { item: index, .. } => Some(index),
            Self::InvalidJson | Self::ItemsNotArray => None,
        }
    }

    /// Returns the rejected source string, when any.
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        match *self {
            Self::InvalidSource { ref source, .. } => Some(source),
            Self::InvalidJson
            | Self::ItemsNotArray
            | Self::ItemNotObject(_)
            | Self::MissingSource(_)
            | Self::SourceNotString(_) => None,
        }
    }

    /// Returns the source rejection reason, when any.
    #[must_use]
    pub const fn source_reason(&self) -> Option<ContextPackSourceError> {
        match *self {
            Self::InvalidSource { reason, .. } => Some(reason),
            Self::InvalidJson
            | Self::ItemsNotArray
            | Self::ItemNotObject(_)
            | Self::MissingSource(_)
            | Self::SourceNotString(_) => None,
        }
    }
}

/// Result of inspecting `context/pack.json` source transparency.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContextPackReport {
    issues: Vec<ContextPackIssue>,
}

impl_issue_report!(ContextPackReport, ContextPackIssue);
