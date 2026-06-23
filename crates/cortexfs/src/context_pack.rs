pub use crate::context_pack_build::{ContextPackBuildError, rebuild_context_pack};
pub use crate::context_pack_inspect::inspect_context_pack_json;
pub use crate::context_pack_source::{ContextPackSourceError, validate_context_pack_source};

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

/// One source selected into a rebuilt context pack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPackBuiltItem {
    kind: String,
    source: String,
    range: Option<String>,
    tokens: u64,
}

impl ContextPackBuiltItem {
    /// Creates a selected pack item.
    #[must_use]
    pub fn new(kind: &str, source: &str, range: Option<String>, tokens: u64) -> Self {
        Self {
            kind: kind.to_owned(),
            source: source.to_owned(),
            range,
            tokens,
        }
    }

    /// Returns the pack item kind.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// Returns the session-relative source.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the optional item range.
    #[must_use]
    pub fn range(&self) -> Option<&str> {
        self.range.as_deref()
    }

    /// Returns the approximate token count used for pack budgeting.
    #[must_use]
    pub const fn tokens(&self) -> u64 {
        self.tokens
    }
}

/// Result of rebuilding `context/pack.json` and `context/pack.md`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContextPackBuild {
    items: Vec<ContextPackBuiltItem>,
    pack_json: String,
    pack_md: String,
}

impl ContextPackBuild {
    /// Creates a context pack build result.
    #[must_use]
    pub const fn new(items: Vec<ContextPackBuiltItem>, pack_json: String, pack_md: String) -> Self {
        Self {
            items,
            pack_json,
            pack_md,
        }
    }

    /// Returns the selected pack items.
    #[must_use]
    pub fn items(&self) -> &[ContextPackBuiltItem] {
        &self.items
    }

    /// Returns the generated `context/pack.json` body.
    #[must_use]
    pub fn pack_json(&self) -> &str {
        &self.pack_json
    }

    /// Returns the generated `context/pack.md` body.
    #[must_use]
    pub fn pack_md(&self) -> &str {
        &self.pack_md
    }
}
