use std::path::{Path, PathBuf};

/// Runtime-owned child cancellation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnedChildCancellationError {
    /// Parent agent name is not a valid object name.
    InvalidParentName,
    /// Child agent name is not a valid object name.
    InvalidChildName,
    /// The child session directory is missing durable history files.
    MissingChildHistory,
    /// The parent session event log is missing.
    MissingParentEvents,
    /// Session state or event files could not be updated.
    CannotRecord,
}

/// Stable parent-side child coordination status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildContextStatus {
    /// Parent has prepared handoff context but the child has not completed.
    Pending,
    /// Child runtime is active.
    Active,
    /// Child returned a result successfully.
    Done,
    /// Child failed and returned an inspectable error result.
    Error,
    /// Child runtime was cancelled; history remains durable.
    Cancelled,
}

impl ChildContextStatus {
    /// Parses a stable child context status value.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "active" => Some(Self::Active),
            "done" => Some(Self::Done),
            "error" => Some(Self::Error),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    /// Returns the stable status word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Done => "done",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Parent-side child handoff/result recording failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildContextRecordError {
    /// Child coordination directory name is not a valid object name.
    InvalidChildName,
    /// Agent name stored in `context/child/<child>/agent` is invalid.
    InvalidAgentName,
    /// Child session name is invalid.
    InvalidSessionName,
    /// Child status is not valid for the requested transition.
    InvalidStatus,
    /// Handoff or result text contains a NUL byte.
    InvalidText,
    /// `refs.jsonl` is not valid stable context refs JSONL.
    InvalidRefs,
    /// Parent session or its context directory is missing required files.
    MissingParentSession,
    /// Child coordination files could not be written.
    CannotRecord,
}

/// Receipt for one exclusively published parent-side child handoff.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChildHandoffReceipt {
    pub(crate) path: PathBuf,
    pub(crate) dev: u64,
    pub(crate) ino: u64,
    pub(crate) guard: Option<String>,
}

impl ChildHandoffReceipt {
    /// Returns the published child channel directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl ChildContextRecordError {
    /// Returns a stable errno name for this child context recording failure.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidChildName
            | Self::InvalidAgentName
            | Self::InvalidSessionName
            | Self::InvalidStatus
            | Self::InvalidText
            | Self::InvalidRefs => "EINVAL",
            Self::MissingParentSession => "ENOENT",
            Self::CannotRecord => "EIO",
        }
    }
}

impl OwnedChildCancellationError {
    /// Returns a stable errno name for this cancellation failure.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidParentName | Self::InvalidChildName => "EINVAL",
            Self::MissingChildHistory | Self::MissingParentEvents => "ENOENT",
            Self::CannotRecord => "EIO",
        }
    }
}

/// Canonical events emitted when parent death cancels an owned child runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedChildCancellationEvents {
    pub(crate) parent_event: String,
    pub(crate) child_event: String,
}

impl OwnedChildCancellationEvents {
    /// Returns the parent session event.
    #[must_use]
    pub fn parent_event(&self) -> &str {
        &self.parent_event
    }

    /// Returns the child session event.
    #[must_use]
    pub fn child_event(&self) -> &str {
        &self.child_event
    }

    /// Returns both events as canonical JSONL.
    #[must_use]
    pub fn jsonl(&self) -> String {
        format!("{}\n{}\n", self.parent_event, self.child_event)
    }
}
