use crate::*;
use std::collections::BTreeSet;

/// Runtime Unix identity used for Linux permission checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentUnixIdentity {
    uid: u32,
    gid: u32,
    groups: Vec<u32>,
}

/// Derived launch/view state for one `agent/<name>.d/` control directory.
///
/// This is a pure filesystem ABI derivation. It does not start a process,
/// create namespaces, execute tools, or interpret MCP/skill/prompt formats.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentRuntimeView {
    pub(crate) agent_name: String,
    pub(crate) control_dir: PathBuf,
    pub(crate) ctx_root: PathBuf,
    pub(crate) ctx_home: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) owner: u32,
    pub(crate) identity: AgentUnixIdentity,
    pub(crate) permissions: AgentPermissions,
    pub(crate) label: String,
    pub(crate) policy_subject: String,
    pub(crate) iso: String,
    pub(crate) parent: Option<String>,
    pub(crate) lifecycle: ChildLifecycle,
    pub(crate) root: PathBuf,
    pub(crate) cwd: PathBuf,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) tool_path: ToolPath,
    pub(crate) mount_table: MountTable,
    pub(crate) model: String,
    pub(crate) model_limit: ModelContextLimit,
    pub(crate) window_setting: AgentWindowSetting,
    pub(crate) effective_window: AgentEffectiveWindow,
    pub(crate) policy: PolicyV0,
    pub(crate) declared_tools: BTreeSet<String>,
    pub(crate) approval: AgentApprovalMode,
}

/// Hosted direct-native tool approval mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentApprovalMode {
    /// Execute fully authorized calls without an interactive approval exchange.
    Auto,
    /// Require one same-socket approval for each fully authorized call.
    Ask,
}

/// Error while deriving an agent runtime view from `agent/<name>.d/*`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentRuntimeViewError {
    /// Agent name is not a valid object name.
    InvalidAgentName,
    /// `agent/<name>.d/` is missing or is not a directory.
    MissingControlDirectory,
    /// A required control file is missing.
    MissingControlFile(String),
    /// A control file could not be read.
    CannotReadControl(String),
    /// A control file has malformed content.
    InvalidControlFile(String),
}

impl AgentUnixIdentity {
    /// Creates an identity from uid, primary gid, and supplementary groups.
    #[must_use]
    pub fn new(uid: u32, gid: u32, groups: impl IntoIterator<Item = u32>) -> Self {
        Self {
            uid,
            gid,
            groups: groups.into_iter().collect(),
        }
    }

    /// Returns the runtime uid.
    #[must_use]
    pub const fn uid(&self) -> u32 {
        self.uid
    }

    /// Returns the runtime primary gid.
    #[must_use]
    pub const fn gid(&self) -> u32 {
        self.gid
    }

    /// Returns supplementary groups.
    #[must_use]
    pub fn groups(&self) -> &[u32] {
        &self.groups
    }

    pub(crate) fn is_in_group(&self, gid: u32) -> bool {
        self.gid == gid || self.groups.contains(&gid)
    }
}

impl AgentRuntimeView {
    /// Returns the agent object name.
    #[must_use]
    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }

    /// Returns the control directory that produced this view.
    #[must_use]
    pub fn control_dir(&self) -> &Path {
        &self.control_dir
    }

    /// Returns `CTX_ROOT`.
    #[must_use]
    pub fn ctx_root(&self) -> &Path {
        &self.ctx_root
    }

    /// Returns `CTX_HOME`.
    #[must_use]
    pub fn ctx_home(&self) -> &Path {
        &self.ctx_home
    }

    /// Returns the agent `HOME`.
    #[must_use]
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Returns the owning Linux uid from `owner`.
    #[must_use]
    pub const fn owner(&self) -> u32 {
        self.owner
    }

    /// Returns the runtime Linux identity from `uid/gid/groups`.
    #[must_use]
    pub const fn identity(&self) -> &AgentUnixIdentity {
        &self.identity
    }

    /// Returns the coarse `r/w/x` file and shell permission ceiling.
    #[must_use]
    pub const fn permissions(&self) -> AgentPermissions {
        self.permissions
    }

    /// Returns the full label control value.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the v0 policy subject used for effective-authority checks.
    #[must_use]
    pub fn policy_subject(&self) -> &str {
        &self.policy_subject
    }

    /// Returns the isolation profile.
    #[must_use]
    pub fn iso(&self) -> &str {
        &self.iso
    }

    /// Returns the optional parent reference.
    #[must_use]
    pub fn parent(&self) -> Option<&str> {
        self.parent.as_deref()
    }

    /// Returns the stable lifecycle value.
    #[must_use]
    pub const fn lifecycle(&self) -> ChildLifecycle {
        self.lifecycle
    }

    /// Returns the chroot root from `root`.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the startup cwd inside the chroot.
    #[must_use]
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Returns the computed environment in process insertion order.
    #[must_use]
    pub fn env(&self) -> &[(String, String)] {
        &self.env
    }

    /// Returns the derived `CTX_PATH` tool lookup path.
    #[must_use]
    pub const fn tool_path(&self) -> &ToolPath {
        &self.tool_path
    }

    /// Returns the parsed mount table.
    #[must_use]
    pub const fn mount_table(&self) -> &MountTable {
        &self.mount_table
    }

    /// Returns the selected model object name.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Returns the trusted hard limit for the selected model independently of
    /// the Agent's durable and effective window selections.
    #[must_use]
    pub const fn model_limit(&self) -> ModelContextLimit {
        self.model_limit
    }

    /// Returns the durable Agent window setting.
    #[must_use]
    pub const fn window_setting(&self) -> AgentWindowSetting {
        self.window_setting
    }

    /// Returns the effective token window after model-limit resolution.
    #[must_use]
    pub const fn effective_window(&self) -> AgentEffectiveWindow {
        self.effective_window
    }

    /// Returns the parsed v0 policy.
    #[must_use]
    pub const fn policy(&self) -> &PolicyV0 {
        &self.policy
    }

    /// Returns the statically declared direct-native tools.
    #[must_use]
    pub const fn declared_tools(&self) -> &BTreeSet<String> {
        &self.declared_tools
    }

    /// Returns the hosted direct-native approval mode.
    #[must_use]
    pub const fn approval(&self) -> AgentApprovalMode {
        self.approval
    }
}

impl AgentRuntimeViewError {
    /// Returns a stable errno name for this derivation failure.
    #[must_use]
    pub const fn errno(&self) -> &'static str {
        match self {
            &Self::InvalidAgentName | &Self::InvalidControlFile(_) => "EINVAL",
            &Self::MissingControlDirectory | &Self::MissingControlFile(_) => "ENOENT",
            &Self::CannotReadControl(_) => "EIO",
        }
    }
}
