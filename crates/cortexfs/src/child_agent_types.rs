/// v1 child lifecycle value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildLifecycle {
    /// Parent-owned durable child. The parent owns cancellation and history.
    Owned,
    /// Parent-owned temporary child. Runtime may remove the agent object on exit.
    Temp,
}

impl ChildLifecycle {
    /// Parses `agent/<child>.d/life`.
    pub fn parse(value: &str) -> Result<Self, ChildAgentDenial> {
        match value.trim() {
            "owned" => Ok(Self::Owned),
            "temp" => Ok(Self::Temp),
            _ => Err(ChildAgentDenial::UnsupportedLifecycle),
        }
    }
}

/// Child control values that carry attenuable authority.
#[derive(Clone, Copy, Debug)]
pub struct ChildAgentControls<'a> {
    identity: &'a AgentUnixIdentity,
    subject: &'a str,
    policy: &'a PolicyV0,
    mounts: &'a MountTable,
}

impl<'a> ChildAgentControls<'a> {
    /// Creates child control values from `uid/gid/groups`, `label`, `policy`,
    /// and `mount`.
    #[must_use]
    pub const fn new(
        identity: &'a AgentUnixIdentity,
        subject: &'a str,
        policy: &'a PolicyV0,
        mounts: &'a MountTable,
    ) -> Self {
        Self {
            identity,
            subject,
            policy,
            mounts,
        }
    }
}

/// Child-agent creation or validation request.
#[derive(Clone, Copy, Debug)]
pub struct ChildAgentRequest<'a> {
    child_name: &'a str,
    parent_ref: &'a str,
    lifecycle: ChildLifecycle,
    controls: ChildAgentControls<'a>,
}

impl<'a> ChildAgentRequest<'a> {
    /// Creates a child request from ordinary child agent control values.
    #[must_use]
    pub const fn new(
        child_name: &'a str,
        parent_ref: &'a str,
        lifecycle: ChildLifecycle,
        controls: ChildAgentControls<'a>,
    ) -> Self {
        Self {
            child_name,
            parent_ref,
            lifecycle,
            controls,
        }
    }
}

/// Parent effective authority used to attenuate a child agent.
#[derive(Clone, Copy, Debug)]
pub struct ChildAgentAuthority<'a> {
    parent_agent: &'a str,
    identity: &'a AgentUnixIdentity,
    subject: &'a str,
    effective_policy: &'a PolicyV0,
    visible_mounts: &'a MountTable,
}

impl<'a> ChildAgentAuthority<'a> {
    /// Creates a parent authority context for child attenuation.
    #[must_use]
    pub const fn new(
        parent_agent: &'a str,
        identity: &'a AgentUnixIdentity,
        subject: &'a str,
        effective_policy: &'a PolicyV0,
        visible_mounts: &'a MountTable,
    ) -> Self {
        Self {
            parent_agent,
            identity,
            subject,
            effective_policy,
            visible_mounts,
        }
    }
}

/// Child-agent attenuation refusal reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildAgentDenial {
    /// Child agent name is not a valid object name.
    InvalidChildName,
    /// Parent agent name is not a valid object name.
    InvalidParentName,
    /// Child subject or parent subject is not a valid policy subject token.
    InvalidSubject,
    /// `agent/<child>.d/parent` does not point at the creating parent.
    ParentMismatch,
    /// Parent reference syntax is invalid.
    InvalidParentRef,
    /// Child lifecycle is not a supported v1 value.
    UnsupportedLifecycle,
    /// Child uid or gid differs from the parent without supervisor authority.
    IdentityExpansion,
    /// Child supplementary groups are not a subset of the parent's groups.
    GroupExpansion,
    /// Child policy grants authority the parent subject does not have.
    PolicyExpansion,
    /// Child mount table exposes paths or permissions outside the parent view.
    MountExpansion,
}

/// Runtime-owned child cancellation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnedChildCancellationError {
    /// Parent agent name is not a valid v1 object name.
    InvalidParentName,
    /// Child agent name is not a valid v1 object name.
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
    /// Child result status is not a terminal result status.
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
    parent_event: String,
    child_event: String,
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
