use crate::{
    abi::authority::{AgentPermissions, ChildLifecycle},
    agent::{
        loopconfig::AgentLoop,
        window::{AgentEffectiveWindow, AgentWindowSetting},
    },
    authority::AgentUnixIdentity,
    mount::table::MountTable,
    policy::PolicyV0,
    provider::model::ModelContextLimit,
    support::toolpath::ToolPath,
};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

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
    pub(crate) model_recommended: ModelContextLimit,
    pub(crate) model_compact: ModelContextLimit,
    pub(crate) window_setting: AgentWindowSetting,
    pub(crate) effective_window: AgentEffectiveWindow,
    pub(crate) compact_setting: AgentWindowSetting,
    pub(crate) effective_compact: AgentEffectiveWindow,
    pub(crate) loop_kind: AgentLoop,
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

macro_rules! borrowed_getters {
    ($( $(#[$doc:meta])* $field:ident: $ty:ty; )*) => {
        $(
            $(#[$doc])*
            #[must_use]
            pub fn $field(&self) -> &$ty {
                &self.$field
            }
        )*
    };
}

macro_rules! copied_getters {
    ($( $(#[$doc:meta])* $field:ident: $ty:ty; )*) => {
        $(
            $(#[$doc])*
            #[must_use]
            pub const fn $field(&self) -> $ty {
                self.$field
            }
        )*
    };
}

impl AgentRuntimeView {
    borrowed_getters! {
        /// Returns the agent object name.
        agent_name: str;
        /// Returns the control directory that produced this view.
        control_dir: Path;
        /// Returns `CTX_ROOT`.
        ctx_root: Path;
        /// Returns `CTX_HOME`.
        ctx_home: Path;
        /// Returns the agent `HOME`.
        home: Path;
        /// Returns the full label control value.
        label: str;
        /// Returns the v0 policy subject used for effective-authority checks.
        policy_subject: str;
        /// Returns the isolation profile.
        iso: str;
        /// Returns the chroot root from `root`.
        root: Path;
        /// Returns the startup cwd inside the chroot.
        cwd: Path;
        /// Returns the computed environment in process insertion order.
        env: [(String, String)];
        /// Returns the selected model object name.
        model: str;
        /// Returns the configured provider-neutral behavior loop.
        loop_kind: AgentLoop;
    }

    copied_getters! {
        /// Returns the owning Linux uid from `owner`.
        owner: u32;
        /// Returns the coarse `r/w/x` file and shell permission ceiling.
        permissions: AgentPermissions;
        /// Returns the stable lifecycle value.
        lifecycle: ChildLifecycle;
        /// Returns the trusted hard limit for the selected model independently of
        /// the Agent's durable and effective window selections.
        model_limit: ModelContextLimit;
        /// Returns the model metadata recommendation before Agent overrides.
        model_recommended: ModelContextLimit;
        /// Returns the model metadata compaction threshold before Agent overrides.
        model_compact: ModelContextLimit;
        /// Returns the durable Agent window setting.
        window_setting: AgentWindowSetting;
        /// Returns the effective token window after model-limit resolution.
        effective_window: AgentEffectiveWindow;
        /// Returns the durable Agent compaction setting.
        compact_setting: AgentWindowSetting;
        /// Returns the effective compaction threshold.
        effective_compact: AgentEffectiveWindow;
        /// Returns the hosted direct-native approval mode.
        approval: AgentApprovalMode;
    }

    /// Returns the runtime Linux identity from `uid/gid/groups`.
    #[must_use]
    pub const fn identity(&self) -> &AgentUnixIdentity {
        &self.identity
    }

    /// Returns the optional parent reference.
    #[must_use]
    pub fn parent(&self) -> Option<&str> {
        self.parent.as_deref()
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
