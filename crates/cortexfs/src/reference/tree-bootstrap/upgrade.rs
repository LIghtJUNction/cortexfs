use super::*;

/// Monotonic reference-tree generation written to the backing source.
pub const REFERENCE_TREE_VERSION: u32 = 1;

/// Relative path for bootstrap state under the source root.
pub const BOOTSTRAP_STATE_REL: &str = "bin/cortexfs.bootstrap.json";

/// Agents formerly shipped by the reference tree and no longer installed.
pub const RETIRED_REFERENCE_AGENTS: &[&str] = &["base", "worker", "executor"];

/// Migration id recording that retired reference agents were reviewed.
pub const MIGRATION_RETIRED_AGENTS_V1: &str = "retired-agents-v1";

/// Planned upgrade / GC action for dry-run and apply reporting.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BootstrapAction {
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
    /// Reference tree generation.
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
    let mut plan = BootstrapPlan {
        actions: Vec::new(),
        current_version: state.as_ref().map(|value| value.tree_version),
        target_version: REFERENCE_TREE_VERSION,
        applied_migrations: state
            .as_ref()
            .map(|value| value.applied_migrations.clone())
            .unwrap_or_default(),
    };

    for agent in REFERENCE_AGENTS {
        let exec = root.join("agent").join(agent.name);
        let control = root.join("agent").join(format!("{}.d", agent.name));
        if !(exec.exists() || control.exists()) {
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
    if plan
        .actions
        .iter()
        .any(|action| matches!(action, BootstrapAction::WriteState { .. }))
    {
        write_bootstrap_state(root, &[MIGRATION_RETIRED_AGENTS_V1])?;
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

fn retired_reference_agent_present(root: &Path, name: &str) -> bool {
    let exec = root.join("agent").join(name);
    let control = root.join("agent").join(format!("{name}.d"));
    let sock = root.join("agent").join(format!("{name}.sock"));
    exec.exists() || control.exists() || sock.exists()
}

/// Returns true when the agent executable looks like a CortexFS-managed wrapper.
#[must_use]
pub fn is_managed_reference_agent_wrapper(path: &Path) -> bool {
    let Ok(content) = plain_fs::read_small_text_file(path, MAX_REFERENCE_SESSION_META_BYTES) else {
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
    let content = plain_fs::read_small_text_file(&path, MAX_REFERENCE_SESSION_META_BYTES).ok()?;
    serde_json::from_str(&content).ok()
}

/// Writes bootstrap state after a successful materialize / GC pass.
pub fn write_bootstrap_state(
    root: &Path,
    new_migrations: &[&str],
) -> Result<(), ReferenceTreeError> {
    let mut applied = read_bootstrap_state(root)
        .map(|state| state.applied_migrations)
        .unwrap_or_default();
    for migration in new_migrations {
        if !applied.iter().any(|value| value == migration) {
            applied.push((*migration).to_owned());
        }
    }
    let state = BootstrapState {
        schema: 1,
        tree_version: REFERENCE_TREE_VERSION,
        managed_agents: REFERENCE_AGENTS
            .iter()
            .map(|agent| agent.name.to_owned())
            .collect(),
        applied_migrations: applied,
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
            root.join("agent").join(name).exists()
                || root.join("agent").join(format!("{name}.d")).exists()
                || root.join("agent").join(format!("{name}.sock")).exists()
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
        && state
            .applied_migrations
            .iter()
            .any(|migration| migration == MIGRATION_RETIRED_AGENTS_V1)
}
