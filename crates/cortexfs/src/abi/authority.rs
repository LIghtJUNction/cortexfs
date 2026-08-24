use crate::policy::PolicyPermission;

/// Coarse agent file and shell permissions projected as Unix owner mode bits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AgentPermissions(pub(crate) u8);
impl AgentPermissions {
    pub const ALL: Self = Self(0o7);
    const CONTROL: [&'static str; 8] = [
        "---\n", "--x\n", "-w-\n", "-wx\n", "r--\n", "r-x\n", "rw-\n", "rwx\n",
    ];
    /// Parses the canonical `rwx` control line.
    #[must_use]
    pub fn parse_control(content: &str) -> Option<Self> {
        let bits = Self::CONTROL.iter().position(|value| *value == content)?;
        u8::try_from(bits).ok().map(Self)
    }
    /// Returns the canonical control value including its final newline.
    #[must_use]
    pub fn control(self) -> &'static str {
        Self::CONTROL
            .get(usize::from(self.0))
            .copied()
            .unwrap_or("---\n")
    }
    /// Returns the owner mode bits rendered by `ls -l` for the permission marker.
    #[must_use]
    pub fn mode(self) -> u32 {
        u32::from(self.0) << 6
    }
    fn tool_bit(name: &str) -> u8 {
        match name {
            "fs.read" | "fs.list" | "fs.stat" => 4,
            "fs.write" | "fs.replace" => 2,
            "shell.exec" | "bash" | "tmux" | "zellij" => 1,
            _ => 0,
        }
    }
    /// Derives the least coarse ceiling needed by a declared tool set.
    #[must_use]
    pub fn for_tools<'a>(tools: impl IntoIterator<Item = &'a str>) -> Self {
        Self(
            tools
                .into_iter()
                .map(Self::tool_bit)
                .fold(0, std::ops::BitOr::bitor),
        )
    }
    pub(crate) fn allows_tool(self, name: &str) -> bool {
        let required = Self::tool_bit(name);
        required == 0 || self.0 & required != 0
    }
}

/// Effective-authority refusal reason for tool execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolExecutionDenial {
    /// Tool name is not a valid object name.
    InvalidToolName,
    /// No executable tool was found through `CTX_PATH`.
    ToolNotFound,
    /// A `CTX_PATH` directory could not be read.
    CannotReadToolPath,
    /// Tool metadata could not be read.
    CannotInspectTool,
    /// Linux uid/gid/groups/mode bits refuse execution.
    LinuxPermission,
    /// No mount entry exposes the selected tool path in the agent view.
    NotMounted,
    /// The selected mount is `noexec`.
    NoExecMount,
    /// Agent policy does not allow `tool:<name> execute`.
    AgentPolicy,
    /// Tool policy does not allow `tool:<name> execute`.
    ToolPolicy,
    /// Agent `perm` mode does not allow this file or shell capability.
    AgentPermission,
    /// Model principals may emit tool-call syntax but must not execute tools.
    ModelCannotExecute,
}

impl ToolExecutionDenial {
    /// Returns a stable errno name for this denial.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidToolName => "EINVAL",
            Self::ToolNotFound => "ENOENT",
            Self::CannotReadToolPath | Self::CannotInspectTool => "EIO",
            Self::LinuxPermission
            | Self::NotMounted
            | Self::NoExecMount
            | Self::AgentPolicy
            | Self::ToolPolicy
            | Self::AgentPermission
            | Self::ModelCannotExecute => "EACCES",
        }
    }
}

/// Stable principal class requesting tool execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolExecutionPrincipal {
    /// Policy-bound agent orchestrator.
    Agent,
    /// Pure inference model endpoint.
    Model,
}

/// Shared-space operation kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedAccess {
    /// Read from a shared space.
    Read,
    /// Write to a shared space.
    Write,
}

impl SharedAccess {
    pub(crate) fn policy_permission(self) -> PolicyPermission {
        match self {
            Self::Read => PolicyPermission::Read,
            Self::Write => PolicyPermission::Write,
        }
    }
}

/// Durable session operation kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionAccess {
    /// Read session history or derived context.
    Read,
    /// Write session history or derived context.
    Write,
    /// Resume a session through the socket protocol.
    Resume,
}

impl SessionAccess {
    pub(crate) fn policy_permission(self) -> PolicyPermission {
        match self {
            Self::Read => PolicyPermission::Read,
            Self::Write => PolicyPermission::Write,
            Self::Resume => PolicyPermission::Resume,
        }
    }

    pub(crate) fn shared_policy_permission(self) -> PolicyPermission {
        match self {
            Self::Read | Self::Resume => PolicyPermission::Read,
            Self::Write => PolicyPermission::Write,
        }
    }
}

/// Effective-authority refusal reason for shared-space access.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedAccessDenial {
    /// Shared-space name is not a valid object name.
    InvalidSharedName,
    /// Path is not a stable shared-space path for the named space.
    WrongSharedPath,
    /// Shared path metadata could not be read.
    CannotInspectPath,
    /// No mount entry exposes the selected shared path in the agent view.
    NotMounted,
    /// A write was requested through a read-only mount.
    ReadOnlyMount,
    /// Linux uid/gid/groups/mode bits refuse access.
    LinuxPermission,
    /// Agent policy does not allow the requested shared-space access.
    Policy,
}

impl SharedAccessDenial {
    /// Returns a stable errno name for this denial.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidSharedName | Self::WrongSharedPath => "EINVAL",
            Self::CannotInspectPath => "EIO",
            Self::ReadOnlyMount => "EROFS",
            Self::NotMounted | Self::LinuxPermission | Self::Policy => "EACCES",
        }
    }
}

/// Effective-authority refusal reason for durable session access.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionAccessDenial {
    /// Path is not a stable private or shared session path.
    InvalidSessionPath,
    /// Session path metadata could not be read.
    CannotInspectPath,
    /// No mount entry exposes the selected session path in the agent view.
    NotMounted,
    /// A write was requested through a read-only mount.
    ReadOnlyMount,
    /// Linux uid/gid/groups/mode bits or private home uid refuse access.
    LinuxPermission,
    /// Shared-space policy does not allow the requested access.
    SharedPolicy,
    /// Session policy does not allow the requested access.
    SessionPolicy,
}

impl SessionAccessDenial {
    /// Returns a stable errno name for this denial.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidSessionPath => "EINVAL",
            Self::CannotInspectPath => "EIO",
            Self::ReadOnlyMount => "EROFS",
            Self::NotMounted | Self::LinuxPermission | Self::SharedPolicy | Self::SessionPolicy => {
                "EACCES"
            }
        }
    }
}

/// Stable child lifecycle value.
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
        Self::parse_exact(value.trim())
    }

    /// Parses an exact wire or tool lifecycle literal without trimming.
    pub(crate) fn parse_exact(value: &str) -> Result<Self, ChildAgentDenial> {
        match value {
            "owned" => Ok(Self::Owned),
            "temp" => Ok(Self::Temp),
            _ => Err(ChildAgentDenial::UnsupportedLifecycle),
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
    /// Child lifecycle is not a supported value.
    UnsupportedLifecycle,
    /// Child uid or gid differs from the parent without supervisor authority.
    IdentityExpansion,
    /// Child supplementary groups are not a subset of the parent's groups.
    GroupExpansion,
    /// Child policy grants authority the parent subject does not have.
    PolicyExpansion,
    /// Child mount table exposes paths or permissions outside the parent view.
    MountExpansion,
    /// Child tool path adds, duplicates, or reorders parent search tiers.
    ToolPathExpansion,
}
