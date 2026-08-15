use crate::*;

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

/// Positive tool execution authority decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolExecutionGrant {
    hit: ToolHit,
}

impl ToolExecutionGrant {
    /// Creates a grant for a concrete `CTX_PATH` hit.
    #[must_use]
    pub const fn new(hit: ToolHit) -> Self {
        Self { hit }
    }
    /// Returns the executable tool selected by left-to-right `CTX_PATH`.
    #[must_use]
    pub const fn hit(&self) -> &ToolHit {
        &self.hit
    }
}

/// Inputs that define an agent's effective authority for a tool execution.
#[derive(Clone, Copy, Debug)]
pub struct ToolExecutionAuthority<'a> {
    pub(crate) principal: ToolExecutionPrincipal,
    pub(crate) identity: &'a AgentUnixIdentity,
    pub(crate) mount_table: &'a MountTable,
    pub(crate) agent_subject: &'a str,
    pub(crate) agent_policy: &'a dyn PolicyEvaluator,
    pub(crate) tool_policy: &'a dyn PolicyEvaluator,
    pub(crate) permissions: AgentPermissions,
}

impl<'a> ToolExecutionAuthority<'a> {
    /// Creates an authority context for one tool execution decision.
    #[must_use]
    pub const fn new(
        identity: &'a AgentUnixIdentity,
        mount_table: &'a MountTable,
        agent_subject: &'a str,
        agent_policy: &'a dyn PolicyEvaluator,
        tool_policy: &'a dyn PolicyEvaluator,
        permissions: AgentPermissions,
    ) -> Self {
        Self {
            principal: ToolExecutionPrincipal::Agent,
            identity,
            mount_table,
            agent_subject,
            agent_policy,
            tool_policy,
            permissions,
        }
    }

    /// Creates an authority context for a model-originated tool execution
    /// attempt. This always denies at the `CortexFS` boundary.
    #[must_use]
    pub const fn model(
        identity: &'a AgentUnixIdentity,
        mount_table: &'a MountTable,
        model_subject: &'a str,
        agent_policy: &'a dyn PolicyEvaluator,
        tool_policy: &'a dyn PolicyEvaluator,
    ) -> Self {
        Self {
            principal: ToolExecutionPrincipal::Model,
            identity,
            mount_table,
            agent_subject: model_subject,
            agent_policy,
            tool_policy,
            permissions: AgentPermissions::ALL,
        }
    }
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

/// Inputs that define an agent's effective authority for shared-space access.
#[derive(Clone, Copy, Debug)]
pub struct SharedAccessAuthority<'a> {
    pub(crate) identity: &'a AgentUnixIdentity,
    pub(crate) mount_table: &'a MountTable,
    pub(crate) agent_subject: &'a str,
    pub(crate) policy: &'a dyn PolicyEvaluator,
}

impl<'a> SharedAccessAuthority<'a> {
    /// Creates an authority context for one shared-space access decision.
    #[must_use]
    pub const fn new(
        identity: &'a AgentUnixIdentity,
        mount_table: &'a MountTable,
        agent_subject: &'a str,
        policy: &'a dyn PolicyEvaluator,
    ) -> Self {
        Self {
            identity,
            mount_table,
            agent_subject,
            policy,
        }
    }
}

/// Inputs that define an agent's effective authority for durable session access.
#[derive(Clone, Copy, Debug)]
pub struct SessionAccessAuthority<'a> {
    pub(crate) identity: &'a AgentUnixIdentity,
    pub(crate) mount_table: &'a MountTable,
    pub(crate) agent_subject: &'a str,
    pub(crate) policy: &'a dyn PolicyEvaluator,
}

impl<'a> SessionAccessAuthority<'a> {
    /// Creates an authority context for one private or shared session access.
    #[must_use]
    pub const fn new(
        identity: &'a AgentUnixIdentity,
        mount_table: &'a MountTable,
        agent_subject: &'a str,
        policy: &'a dyn PolicyEvaluator,
    ) -> Self {
        Self {
            identity,
            mount_table,
            agent_subject,
            policy,
        }
    }
}
