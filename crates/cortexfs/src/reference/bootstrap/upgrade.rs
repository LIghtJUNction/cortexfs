use super::*;

/// Monotonic target version written to the backing source.
pub const REFERENCE_TREE_VERSION: u32 = 8;

/// Relative path for bootstrap state under the source root.
pub const BOOTSTRAP_STATE_REL: &str = "bin/cortexfs.bootstrap.json";

/// Agents formerly shipped by the reference tree and no longer installed.
pub const RETIRED_REFERENCE_AGENTS: &[&str] = &["base", "executor"];

/// Migration id recording that retired reference agents were reviewed.
pub const MIGRATION_RETIRED_AGENTS: &str = "retired-agents";
/// Migration id recording adoption of the rolling reference-tree model.
pub const MIGRATION_ROLLING_TREE: &str = "rolling-tree";
/// Migration id recording installation of the self-update reference tool.
pub const MIGRATION_AGENT_UPDATE: &str = "agent-update";
/// Migration id recording the current default model refresh.
pub const MIGRATION_CURRENT_MODELS: &str = "current-models";
/// Migration id recording the coarse agent permission controls.
pub const MIGRATION_AGENT_PERMISSIONS: &str = "agent-permissions";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ReferenceTreeMigration {
    target_version: u32,
    id: &'static str,
}

const REFERENCE_TREE_MIGRATIONS: &[ReferenceTreeMigration] = &[
    ReferenceTreeMigration {
        target_version: 4,
        id: MIGRATION_RETIRED_AGENTS,
    },
    ReferenceTreeMigration {
        target_version: 5,
        id: MIGRATION_ROLLING_TREE,
    },
    ReferenceTreeMigration {
        target_version: 6,
        id: MIGRATION_AGENT_UPDATE,
    },
    ReferenceTreeMigration {
        target_version: 7,
        id: MIGRATION_CURRENT_MODELS,
    },
    ReferenceTreeMigration {
        target_version: 8,
        id: MIGRATION_AGENT_PERMISSIONS,
    },
];

/// Planned upgrade / GC action for dry-run and apply reporting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BootstrapAction {
    /// Source state was written by a newer binary and cannot be downgraded.
    RejectVersion { current: u32, target: u32 },
    /// Ordered migration required to reach the target tree version.
    ApplyMigration { version: u32, id: &'static str },
    /// Retired agent exists but ownership cannot be proven safely.
    SkipAgent { name: String, reason: String },
    /// Reference agent missing from the source tree.
    EnsureAgent { name: String },
    /// Would write or refresh bootstrap state.
    WriteState { path: String },
}

/// Upgrade plan derived from the current source tree.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct BootstrapPlan {
    /// Planned actions in report order.
    pub actions: Vec<BootstrapAction>,
    /// Version recorded on disk, when present.
    pub current_version: Option<u32>,
    /// Target version this binary installs.
    pub target_version: u32,
    /// Migration ids already recorded on disk.
    pub applied_migrations: Vec<String>,
}

/// Durable bootstrap state stored under `bin/cortexfs.bootstrap.json`.
#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BootstrapState {
    /// Schema of this state file.
    #[serde(default = "bootstrap_state_schema_default")]
    pub schema: u32,
    /// Applied reference-tree version.
    pub tree_version: u32,
    /// Managed reference agent names at last bootstrap.
    #[serde(default)]
    pub managed_agents: Vec<String>,
    /// Applied migration identifiers.
    #[serde(default)]
    pub applied_migrations: Vec<String>,
}

const fn bootstrap_state_schema_default() -> u32 {
    1
}

/// Builds a no-write plan for source tree upgrade / GC.
#[must_use]
pub fn plan_reference_tree_upgrade(root: &Path) -> BootstrapPlan {
    let state = read_bootstrap_state(root);
    let worker_is_managed = state
        .as_ref()
        .is_some_and(|state| state.managed_agents.iter().any(|name| name == "worker"));
    let mut plan = BootstrapPlan {
        actions: Vec::new(),
        current_version: state.as_ref().map(|value| value.tree_version),
        target_version: REFERENCE_TREE_VERSION,
        applied_migrations: state
            .as_ref()
            .map(|value| value.applied_migrations.clone())
            .unwrap_or_default(),
    };

    let current_version = plan.current_version.unwrap_or(0);
    if current_version > REFERENCE_TREE_VERSION {
        plan.actions.push(BootstrapAction::RejectVersion {
            current: current_version,
            target: REFERENCE_TREE_VERSION,
        });
        return plan;
    }
    plan.actions.extend(
        REFERENCE_TREE_MIGRATIONS
            .iter()
            .filter(|migration| {
                current_version < migration.target_version
                    && migration.target_version <= REFERENCE_TREE_VERSION
            })
            .map(|migration| BootstrapAction::ApplyMigration {
                version: migration.target_version,
                id: migration.id,
            }),
    );

    for agent in REFERENCE_AGENTS {
        let exec = cortexfs_paths::agent_path(root, agent.name);
        let control = cortexfs_paths::agent_control_path(root, agent.name);
        let socket = cortexfs_paths::agent_socket_path(root, agent.name);
        let exists = exec.exists() || control.exists() || fs::symlink_metadata(socket).is_ok();
        if agent.name == "worker" && exists && !worker_is_managed {
            plan.actions.push(BootstrapAction::SkipAgent {
                name: agent.name.to_owned(),
                reason: "existing worker requires manual review before reference-tree promotion"
                    .to_owned(),
            });
        } else if !exists {
            plan.actions.push(BootstrapAction::EnsureAgent {
                name: agent.name.to_owned(),
            });
        }
    }

    for name in RETIRED_REFERENCE_AGENTS {
        if retired_reference_agent_present(root, name) {
            plan.actions.push(BootstrapAction::SkipAgent {
                name: (*name).to_owned(),
                reason: "ownership and full control-tree integrity cannot be proven; leaving for manual review"
                    .to_owned(),
            });
        }
    }

    if !state.as_ref().is_some_and(bootstrap_state_matches_target) {
        plan.actions.push(BootstrapAction::WriteState {
            path: BOOTSTRAP_STATE_REL.to_owned(),
        });
    }
    plan
}

/// Applies the safe reference-tree upgrade subset and refreshes state if needed.
pub fn apply_reference_tree_upgrade(root: &Path) -> Result<BootstrapPlan, ReferenceTreeError> {
    let plan = plan_reference_tree_upgrade(root);
    reject_unsupported_version(&plan)?;
    if plan.actions.iter().any(|action| {
        matches!(action, BootstrapAction::EnsureAgent { name }
            if REFERENCE_AGENTS.iter().any(|agent| agent.name == name))
    }) {
        return Ok(plan);
    }
    apply_precomputed_reference_tree_upgrade(root, plan)
}

pub(crate) fn apply_precomputed_reference_tree_upgrade(
    root: &Path,
    plan: BootstrapPlan,
) -> Result<BootstrapPlan, ReferenceTreeError> {
    reject_unsupported_version(&plan)?;
    let skips_current_agent = plan.actions.iter().any(|action| {
        matches!(action, BootstrapAction::SkipAgent { name, .. }
            if REFERENCE_AGENTS.iter().any(|agent| agent.name == name))
    });
    if !skips_current_agent
        && plan
            .actions
            .iter()
            .any(|action| matches!(action, BootstrapAction::WriteState { .. }))
    {
        write_bootstrap_state(root)?;
    }
    Ok(plan)
}

/// Formats plan lines for CLI dry-run / check output.
#[must_use]
pub fn format_bootstrap_plan_lines(plan: &BootstrapPlan) -> Vec<String> {
    let mut lines = vec![
        format!(
            "tree_version={}->{}",
            plan.current_version
                .map_or_else(|| "none".to_owned(), |value| value.to_string()),
            plan.target_version
        ),
        format!(
            "migrations={}",
            if plan.applied_migrations.is_empty() {
                "none".to_owned()
            } else {
                plan.applied_migrations.join(",")
            }
        ),
    ];
    for action in &plan.actions {
        match *action {
            BootstrapAction::RejectVersion { current, target } => {
                lines.push(format!(
                    "reject tree_version={current} newer_than_target={target}"
                ));
            }
            BootstrapAction::ApplyMigration { version, id } => {
                lines.push(format!("would_apply migration v{version} {id}"));
            }
            BootstrapAction::SkipAgent {
                ref name,
                ref reason,
            } => {
                lines.push(format!("would_skip agent/{name} ({reason})"));
            }
            BootstrapAction::EnsureAgent { ref name } => {
                lines.push(format!("would_ensure agent/{name}"));
            }
            BootstrapAction::WriteState { ref path } => {
                lines.push(format!("would_write {path}"));
            }
        }
    }
    lines
}

pub(crate) fn reject_unsupported_version(plan: &BootstrapPlan) -> Result<(), ReferenceTreeError> {
    if plan
        .actions
        .iter()
        .any(|action| matches!(action, BootstrapAction::RejectVersion { .. }))
    {
        Err(ReferenceTreeError::UnsupportedVersion)
    } else {
        Ok(())
    }
}

fn retired_reference_agent_present(root: &Path, name: &str) -> bool {
    let exec = cortexfs_paths::agent_path(root, name);
    let control = cortexfs_paths::agent_control_path(root, name);
    let sock = cortexfs_paths::agent_socket_path(root, name);
    exec.exists() || control.exists() || sock.exists()
}

/// Returns true when the agent executable looks like a CortexFS-managed wrapper.
#[must_use]
pub fn is_managed_reference_agent_wrapper(path: &Path) -> bool {
    let Ok(content) = support::plain::read_small_text_file(path, MAX_REFERENCE_SESSION_META_BYTES)
    else {
        return false;
    };
    content.contains("# cortexfs.object=agent")
        || content.contains("CortexFS generated object wrapper")
        || content.contains("CortexFS reference-tree agent stub")
}

/// Reads bootstrap state from the source tree when present.
#[must_use]
pub fn read_bootstrap_state(root: &Path) -> Option<BootstrapState> {
    let path = root.join(BOOTSTRAP_STATE_REL);
    let content =
        support::plain::read_small_text_file(&path, MAX_REFERENCE_SESSION_META_BYTES).ok()?;
    serde_json::from_str(&content).ok()
}

/// Writes bootstrap state after a successful materialize / GC pass.
pub fn write_bootstrap_state(root: &Path) -> Result<(), ReferenceTreeError> {
    if read_bootstrap_state(root).is_some_and(|state| state.tree_version > REFERENCE_TREE_VERSION) {
        return Err(ReferenceTreeError::UnsupportedVersion);
    }
    let state = BootstrapState {
        schema: 1,
        tree_version: REFERENCE_TREE_VERSION,
        managed_agents: REFERENCE_AGENTS
            .iter()
            .map(|agent| agent.name.to_owned())
            .collect(),
        applied_migrations: REFERENCE_TREE_MIGRATIONS
            .iter()
            .filter(|migration| migration.target_version <= REFERENCE_TREE_VERSION)
            .map(|migration| migration.id.to_owned())
            .collect(),
    };
    let content =
        serde_json::to_string_pretty(&state).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    write_reference_text(&root.join(BOOTSTRAP_STATE_REL), &format!("{content}\n"))
}

/// Lists retired agents still present under `agent/` for doctor/check.
#[must_use]
pub fn list_present_retired_reference_agents(root: &Path) -> Vec<String> {
    RETIRED_REFERENCE_AGENTS
        .iter()
        .filter(|name| {
            cortexfs_paths::agent_path(root, name).exists()
                || cortexfs_paths::agent_control_path(root, name).exists()
                || cortexfs_paths::agent_socket_path(root, name).exists()
        })
        .map(|name| (*name).to_owned())
        .collect()
}

/// Returns whether state exactly describes the currently managed reference tree.
#[must_use]
pub fn bootstrap_state_matches_target(state: &BootstrapState) -> bool {
    state.schema == bootstrap_state_schema_default()
        && state.tree_version == REFERENCE_TREE_VERSION
        && state.managed_agents
            == REFERENCE_AGENTS
                .iter()
                .map(|agent| agent.name.to_owned())
                .collect::<Vec<_>>()
}
