use std::collections::HashSet;

use serde::Deserialize;
use serde_json::Value;

use crate::{
    PolicyObjectClass, PolicyPermission, PolicyV0, abi_path::is_model_reference, is_object_name,
};

/// Stable issue found in a parent-session hybrid agent schedule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentScheduleIssue {
    /// Schedule file is not valid JSON.
    InvalidJson,
    /// Top-level schedule value is not a JSON object.
    ScheduleNotObject,
    /// `version` is missing or is not `1`.
    InvalidVersion,
    /// `mode` is missing or is not `dag-react`.
    InvalidMode,
    /// `nodes` is missing, empty, or not an array.
    InvalidNodes,
    /// A node is not a JSON object.
    NodeNotObject { index: usize },
    /// A node id, agent name, child name, kind, dependency, or permission name is malformed.
    InvalidField {
        node: Option<String>,
        field: String,
        value: String,
    },
    /// Two nodes use the same id.
    DuplicateNode { node: String },
    /// Two delegated nodes use the same child result channel.
    DuplicateChild { child: String },
    /// A dependency names no node in this schedule.
    UnknownDependency { node: String, dependency: String },
    /// A completed node name does not exist in this schedule.
    UnknownCompletedNode { node: String },
    /// A delegated node was supplied as a local completion without child result state.
    DelegatedCompletionRequiresChildResult { node: String },
    /// The dependency graph contains a cycle.
    DependencyCycle { node: String },
    /// A `ReAct` node is missing a bounded `max_steps` value.
    InvalidReactBound { node: String },
    /// A delegated node is missing non-empty handoff text.
    MissingHandoff { node: String },
    /// A declared permission is not present in the parent effective policy.
    PermissionNotGranted {
        node: String,
        class: String,
        name: String,
        permission: String,
    },
}

/// Result of inspecting a parent-session hybrid agent schedule.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentScheduleReport {
    issues: Vec<AgentScheduleIssue>,
}

impl_issue_report!(AgentScheduleReport, AgentScheduleIssue);

/// Stable hybrid schedule node kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentScheduleNodeKind {
    /// A deterministic DAG node with no internal tool loop contract.
    Dag,
    /// A bounded `ReAct` node with a `max_steps` loop limit.
    React,
}

impl AgentScheduleNodeKind {
    /// Parses a stable schedule node kind.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "dag" => Some(Self::Dag),
            "react" => Some(Self::React),
            _ => None,
        }
    }

    /// Returns the stable JSON word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dag => "dag",
            Self::React => "react",
        }
    }
}

/// Validated parent-session schedule node.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentScheduleNode {
    id: String,
    kind: AgentScheduleNodeKind,
    agent: String,
    child: Option<String>,
    child_session: Option<String>,
    handoff: Option<String>,
    deps: Vec<String>,
    max_steps: Option<u64>,
}

impl AgentScheduleNode {
    /// Returns the schedule node id.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Returns the stable node kind.
    #[must_use]
    pub const fn kind(&self) -> AgentScheduleNodeKind {
        self.kind
    }

    /// Returns the agent intended to execute this node.
    #[must_use]
    pub fn agent(&self) -> &str {
        &self.agent
    }

    /// Returns the parent-owned child result channel, when delegated.
    #[must_use]
    pub fn child(&self) -> Option<&str> {
        self.child.as_deref()
    }

    /// Returns the child session name, when delegated.
    #[must_use]
    pub fn child_session(&self) -> Option<&str> {
        self.child_session.as_deref()
    }

    /// Returns handoff text for delegated child nodes.
    #[must_use]
    pub fn handoff(&self) -> Option<&str> {
        self.handoff.as_deref()
    }

    /// Returns dependency node ids.
    #[must_use]
    pub fn deps(&self) -> &[String] {
        &self.deps
    }

    /// Returns the `ReAct` loop bound, when this is a `react` node.
    #[must_use]
    pub const fn max_steps(&self) -> Option<u64> {
        self.max_steps
    }
}

/// Ready delegated child handoff derived from a validated schedule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentScheduleChildHandoff {
    node: String,
    child: String,
    agent: String,
    session: String,
    handoff: String,
}

impl AgentScheduleChildHandoff {
    /// Returns the schedule node id that produced this handoff.
    #[must_use]
    pub fn node(&self) -> &str {
        &self.node
    }

    /// Returns the parent-owned child result channel name.
    #[must_use]
    pub fn child(&self) -> &str {
        &self.child
    }

    /// Returns the child agent object name.
    #[must_use]
    pub fn agent(&self) -> &str {
        &self.agent
    }

    /// Returns the child session name.
    #[must_use]
    pub fn session(&self) -> &str {
        &self.session
    }

    /// Returns the handoff markdown body.
    #[must_use]
    pub fn handoff(&self) -> &str {
        &self.handoff
    }
}

/// Result of advancing a parent-owned hybrid schedule from session state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentScheduleAdvance {
    completed_nodes: Vec<String>,
    handoffs: Vec<AgentScheduleChildHandoff>,
}

impl AgentScheduleAdvance {
    /// Creates an advance result.
    #[must_use]
    pub const fn new(
        completed_nodes: Vec<String>,
        handoffs: Vec<AgentScheduleChildHandoff>,
    ) -> Self {
        Self {
            completed_nodes,
            handoffs,
        }
    }

    /// Returns completed node ids known during this advance.
    #[must_use]
    pub fn completed_nodes(&self) -> &[String] {
        &self.completed_nodes
    }

    /// Returns delegated handoffs materialized during this advance.
    #[must_use]
    pub fn handoffs(&self) -> &[AgentScheduleChildHandoff] {
        &self.handoffs
    }
}

/// Failure while recording a parent-owned hybrid schedule to session context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentScheduleRecordError {
    /// Schedule text contains a raw NUL byte.
    InvalidText,
    /// Schedule JSON did not pass [`inspect_agent_schedule_json`].
    InvalidSchedule(AgentScheduleReport),
    /// Parent session or its `context/` directory is missing or unsafe.
    MissingParentSession,
    /// `context/plan.json` could not be atomically replaced.
    CannotRecord,
}

impl AgentScheduleRecordError {
    /// Returns a stable errno name for this schedule recording failure.
    #[must_use]
    pub const fn errno(&self) -> &'static str {
        match *self {
            Self::InvalidText | Self::InvalidSchedule(_) => "EINVAL",
            Self::MissingParentSession => "ENOENT",
            Self::CannotRecord => "EIO",
        }
    }
}

#[derive(Deserialize)]
struct ScheduleJson {
    version: Option<Value>,
    mode: Option<Value>,
    nodes: Option<Value>,
}

#[derive(Deserialize)]
struct ScheduleNodeJson {
    id: Option<Value>,
    kind: Option<Value>,
    agent: Option<Value>,
    child: Option<Value>,
    session: Option<Value>,
    handoff: Option<Value>,
    deps: Option<Value>,
    max_steps: Option<Value>,
    requires: Option<Value>,
}

#[derive(Deserialize)]
struct SchedulePermissionJson {
    class: Option<Value>,
    name: Option<Value>,
    permission: Option<Value>,
}

struct ScheduleInspectContext<'a> {
    parent_subject: &'a str,
    parent_policy: &'a PolicyV0,
}

#[derive(Default)]
struct ScheduleSeen {
    nodes: HashSet<String>,
    children: HashSet<String>,
}

/// Inspects a hybrid DAG/ReAct schedule stored as ordinary parent session
/// context, for example `context/plan.json`.
///
/// This is a pure validation helper. It does not create agents, execute tools,
/// watch files, or add a scheduler namespace. The parent agent remains
/// responsible for turning accepted nodes into child-agent handoffs at a Git
/// commit boundary.
#[must_use]
pub fn inspect_agent_schedule_json(
    content: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
) -> AgentScheduleReport {
    let mut issues = Vec::new();
    if !is_object_name(parent_subject) {
        issues.push(AgentScheduleIssue::InvalidField {
            node: None,
            field: "parent_subject".to_owned(),
            value: parent_subject.to_owned(),
        });
        return AgentScheduleReport::new(issues);
    }

    let Ok(value) = serde_json::from_str::<Value>(content) else {
        issues.push(AgentScheduleIssue::InvalidJson);
        return AgentScheduleReport::new(issues);
    };
    if !value.is_object() {
        issues.push(AgentScheduleIssue::ScheduleNotObject);
        return AgentScheduleReport::new(issues);
    }
    let Ok(schedule) = serde_json::from_value::<ScheduleJson>(value) else {
        issues.push(AgentScheduleIssue::InvalidJson);
        return AgentScheduleReport::new(issues);
    };
    if !matches!(schedule.version.as_ref().and_then(Value::as_u64), Some(1)) {
        issues.push(AgentScheduleIssue::InvalidVersion);
    }
    if !matches!(
        schedule.mode.as_ref().and_then(Value::as_str),
        Some("dag-react")
    ) {
        issues.push(AgentScheduleIssue::InvalidMode);
    }

    let Some(nodes_value) = schedule.nodes else {
        issues.push(AgentScheduleIssue::InvalidNodes);
        return AgentScheduleReport::new(issues);
    };
    let Some(node_values) = nodes_value.as_array() else {
        issues.push(AgentScheduleIssue::InvalidNodes);
        return AgentScheduleReport::new(issues);
    };
    if node_values.is_empty() {
        issues.push(AgentScheduleIssue::InvalidNodes);
        return AgentScheduleReport::new(issues);
    }

    inspect_schedule_nodes(node_values, parent_subject, parent_policy, &mut issues);

    AgentScheduleReport::new(issues)
}

/// Returns the currently ready nodes from a valid hybrid DAG/ReAct schedule.
///
/// `completed_nodes` are node ids whose durable results are already available
/// to the parent. The returned nodes are not completed and have all deps
/// completed. Order follows the schedule file, so callers can choose how much
/// parallelism to use without another scheduler namespace.
pub fn ready_agent_schedule_nodes(
    content: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
    completed_nodes: &[&str],
) -> Result<Vec<AgentScheduleNode>, AgentScheduleReport> {
    let (nodes, mut issues) =
        parse_valid_agent_schedule_nodes(content, parent_subject, parent_policy);
    if issues.is_empty() {
        inspect_completed_nodes(&nodes, completed_nodes, &mut issues);
    }
    if !issues.is_empty() {
        return Err(AgentScheduleReport::new(issues));
    }
    let completed = completed_nodes.iter().copied().collect::<HashSet<_>>();
    Ok(nodes
        .into_iter()
        .filter(|node| {
            !completed.contains(node.id())
                && node
                    .deps()
                    .iter()
                    .all(|dep| completed.contains(dep.as_str()))
        })
        .collect())
}

/// Returns validated schedule nodes from a hybrid DAG/ReAct schedule.
pub fn agent_schedule_nodes(
    content: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
) -> Result<Vec<AgentScheduleNode>, AgentScheduleReport> {
    let (nodes, issues) = parse_valid_agent_schedule_nodes(content, parent_subject, parent_policy);
    if issues.is_empty() {
        Ok(nodes)
    } else {
        Err(AgentScheduleReport::new(issues))
    }
}

/// Returns ready delegated child handoffs from a valid hybrid schedule.
///
/// This derives parent-owned `context/child/<child>/handoff.md` inputs. It does
/// not create agents, start runtimes, or mark nodes complete.
pub fn ready_agent_schedule_child_handoffs(
    content: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
    completed_nodes: &[&str],
) -> Result<Vec<AgentScheduleChildHandoff>, AgentScheduleReport> {
    Ok(
        ready_agent_schedule_nodes(content, parent_subject, parent_policy, completed_nodes)?
            .into_iter()
            .filter_map(|node| {
                Some(AgentScheduleChildHandoff {
                    node: node.id,
                    child: node.child?,
                    agent: node.agent,
                    session: node.child_session.unwrap_or_else(|| "default".to_owned()),
                    handoff: node.handoff?,
                })
            })
            .collect(),
    )
}

fn parse_valid_agent_schedule_nodes(
    content: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
) -> (Vec<AgentScheduleNode>, Vec<AgentScheduleIssue>) {
    let mut issues = Vec::new();
    if !is_object_name(parent_subject) {
        issues.push(AgentScheduleIssue::InvalidField {
            node: None,
            field: "parent_subject".to_owned(),
            value: parent_subject.to_owned(),
        });
        return (Vec::new(), issues);
    }

    let Ok(value) = serde_json::from_str::<Value>(content) else {
        issues.push(AgentScheduleIssue::InvalidJson);
        return (Vec::new(), issues);
    };
    if !value.is_object() {
        issues.push(AgentScheduleIssue::ScheduleNotObject);
        return (Vec::new(), issues);
    }
    let Ok(schedule) = serde_json::from_value::<ScheduleJson>(value) else {
        issues.push(AgentScheduleIssue::InvalidJson);
        return (Vec::new(), issues);
    };
    if !matches!(schedule.version.as_ref().and_then(Value::as_u64), Some(1)) {
        issues.push(AgentScheduleIssue::InvalidVersion);
    }
    if !matches!(
        schedule.mode.as_ref().and_then(Value::as_str),
        Some("dag-react")
    ) {
        issues.push(AgentScheduleIssue::InvalidMode);
    }

    let Some(nodes_value) = schedule.nodes else {
        issues.push(AgentScheduleIssue::InvalidNodes);
        return (Vec::new(), issues);
    };
    let Some(node_values) = nodes_value.as_array() else {
        issues.push(AgentScheduleIssue::InvalidNodes);
        return (Vec::new(), issues);
    };
    if node_values.is_empty() {
        issues.push(AgentScheduleIssue::InvalidNodes);
        return (Vec::new(), issues);
    }

    let nodes = inspect_schedule_nodes(node_values, parent_subject, parent_policy, &mut issues);
    (nodes, issues)
}

fn inspect_schedule_nodes(
    node_values: &[Value],
    parent_subject: &str,
    parent_policy: &PolicyV0,
    issues: &mut Vec<AgentScheduleIssue>,
) -> Vec<AgentScheduleNode> {
    let mut nodes = Vec::new();
    let mut seen = ScheduleSeen::default();
    let inspect_context = ScheduleInspectContext {
        parent_subject,
        parent_policy,
    };
    for (index, node_value) in node_values.iter().enumerate() {
        inspect_schedule_node(
            index,
            node_value,
            &inspect_context,
            &mut seen,
            &mut nodes,
            issues,
        );
    }
    inspect_schedule_dependencies(&nodes, issues);
    nodes
}

fn inspect_schedule_node(
    index: usize,
    value: &Value,
    context: &ScheduleInspectContext<'_>,
    seen: &mut ScheduleSeen,
    nodes: &mut Vec<AgentScheduleNode>,
    issues: &mut Vec<AgentScheduleIssue>,
) {
    if !value.is_object() {
        issues.push(AgentScheduleIssue::NodeNotObject { index });
        return;
    }
    let Ok(node) = serde_json::from_value::<ScheduleNodeJson>(value.clone()) else {
        issues.push(AgentScheduleIssue::InvalidJson);
        return;
    };
    let id = required_object_name(None, "id", node.id.as_ref(), issues);
    let Some(id) = id else {
        return;
    };
    if !seen.nodes.insert(id.clone()) {
        issues.push(AgentScheduleIssue::DuplicateNode { node: id.clone() });
    }
    let node_ref = Some(id.clone());

    let kind_text = required_word(node_ref.as_ref(), "kind", node.kind.as_ref(), issues);
    let kind = kind_text.as_deref().and_then(AgentScheduleNodeKind::parse);
    if kind.is_none() {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node_ref.clone(),
            field: "kind".to_owned(),
            value: kind_text.unwrap_or_default(),
        });
    }
    let agent = required_object_name(node_ref.as_ref(), "agent", node.agent.as_ref(), issues);
    let child = node
        .child
        .as_ref()
        .and_then(|child| required_object_name(node_ref.as_ref(), "child", Some(child), issues));
    let child_session = node.session.as_ref().and_then(|session| {
        required_object_name(node_ref.as_ref(), "session", Some(session), issues)
    });
    let handoff = node
        .handoff
        .as_ref()
        .and_then(|handoff| required_handoff_text(node_ref.as_ref(), handoff, issues));
    if child.is_none() && child_session.is_some() {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node_ref.clone(),
            field: "session".to_owned(),
            value: "requires child".to_owned(),
        });
    }
    if child.is_none() && handoff.is_some() {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node_ref.clone(),
            field: "handoff".to_owned(),
            value: "requires child".to_owned(),
        });
    }
    if child.is_some() && handoff.is_none() {
        issues.push(AgentScheduleIssue::MissingHandoff { node: id.clone() });
    }
    if let Some(child) = child.as_ref()
        && !seen.children.insert(child.clone())
    {
        issues.push(AgentScheduleIssue::DuplicateChild {
            child: child.clone(),
        });
    }
    let child_session = child
        .as_ref()
        .map(|_child| child_session.unwrap_or_else(|| "default".to_owned()));

    let deps = inspect_string_array(node_ref.as_ref(), "deps", node.deps.as_ref(), issues);
    let max_steps = node.max_steps.as_ref().and_then(Value::as_u64);
    if kind == Some(AgentScheduleNodeKind::React) && !valid_react_bound(node.max_steps.as_ref()) {
        issues.push(AgentScheduleIssue::InvalidReactBound { node: id.clone() });
    }
    if child.is_some()
        && let Some(agent) = agent.as_ref()
        && !context.parent_policy.allows(
            context.parent_subject,
            PolicyObjectClass::Agent,
            agent,
            PolicyPermission::Create,
        )
        && !requires_permission(
            node.requires.as_ref(),
            PolicyObjectClass::Agent,
            agent,
            "create",
        )
    {
        issues.push(AgentScheduleIssue::PermissionNotGranted {
            node: id.clone(),
            class: "agent".to_owned(),
            name: agent.clone(),
            permission: "create".to_owned(),
        });
    }
    inspect_required_permissions(
        &id,
        node.requires.as_ref(),
        context.parent_subject,
        context.parent_policy,
        issues,
    );
    if let (Some(kind), Some(agent)) = (kind, agent) {
        nodes.push(AgentScheduleNode {
            id,
            kind,
            agent,
            child,
            child_session,
            handoff,
            deps,
            max_steps,
        });
    }
}

fn inspect_string_array(
    node: Option<&String>,
    field: &str,
    value: Option<&Value>,
    issues: &mut Vec<AgentScheduleIssue>,
) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(values) = value.as_array() else {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node.cloned(),
            field: field.to_owned(),
            value: value.to_string(),
        });
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in values {
        let Some(value) = item.as_str() else {
            issues.push(AgentScheduleIssue::InvalidField {
                node: node.cloned(),
                field: field.to_owned(),
                value: item.to_string(),
            });
            continue;
        };
        if !is_object_name(value) {
            issues.push(AgentScheduleIssue::InvalidField {
                node: node.cloned(),
                field: field.to_owned(),
                value: value.to_owned(),
            });
            continue;
        }
        out.push(value.to_owned());
    }
    out
}

fn requires_permission(
    value: Option<&Value>,
    expected_class: PolicyObjectClass,
    expected_name: &str,
    expected_permission: &str,
) -> bool {
    let Some(values) = value.and_then(Value::as_array) else {
        return false;
    };
    values.iter().any(|value| {
        let Ok(permission) = serde_json::from_value::<SchedulePermissionJson>(value.clone()) else {
            return false;
        };
        let Some(class_name) = permission.class.as_ref().and_then(Value::as_str) else {
            return false;
        };
        PolicyObjectClass::parse(class_name) == Some(expected_class)
            && permission.name.as_ref().and_then(Value::as_str) == Some(expected_name)
            && permission.permission.as_ref().and_then(Value::as_str) == Some(expected_permission)
    })
}

fn inspect_required_permissions(
    node: &str,
    value: Option<&Value>,
    parent_subject: &str,
    parent_policy: &PolicyV0,
    issues: &mut Vec<AgentScheduleIssue>,
) {
    let Some(value) = value else {
        return;
    };
    let Some(values) = value.as_array() else {
        issues.push(AgentScheduleIssue::InvalidField {
            node: Some(node.to_owned()),
            field: "requires".to_owned(),
            value: value.to_string(),
        });
        return;
    };
    for value in values {
        inspect_required_permission(node, value, parent_subject, parent_policy, issues);
    }
}

fn inspect_required_permission(
    node: &str,
    value: &Value,
    parent_subject: &str,
    parent_policy: &PolicyV0,
    issues: &mut Vec<AgentScheduleIssue>,
) {
    let Ok(permission) = serde_json::from_value::<SchedulePermissionJson>(value.clone()) else {
        issues.push(AgentScheduleIssue::InvalidField {
            node: Some(node.to_owned()),
            field: "requires".to_owned(),
            value: value.to_string(),
        });
        return;
    };
    let Some(class_name) = required_word(
        Some(&node.to_owned()),
        "requires.class",
        permission.class.as_ref(),
        issues,
    ) else {
        return;
    };
    let Some(class) = PolicyObjectClass::parse(&class_name) else {
        issues.push(AgentScheduleIssue::InvalidField {
            node: Some(node.to_owned()),
            field: "requires.class".to_owned(),
            value: class_name,
        });
        return;
    };
    let Some(name) = required_permission_object_name(
        Some(&node.to_owned()),
        "requires.name",
        permission.name.as_ref(),
        class,
        issues,
    ) else {
        return;
    };
    let Some(permission_name) = required_word(
        Some(&node.to_owned()),
        "requires.permission",
        permission.permission.as_ref(),
        issues,
    ) else {
        return;
    };
    let Some(permission) = PolicyPermission::parse_for_class(class, &permission_name) else {
        issues.push(AgentScheduleIssue::InvalidField {
            node: Some(node.to_owned()),
            field: "requires.permission".to_owned(),
            value: permission_name,
        });
        return;
    };
    if !parent_policy.allows(parent_subject, class, &name, permission) {
        issues.push(AgentScheduleIssue::PermissionNotGranted {
            node: node.to_owned(),
            class: class_name,
            name,
            permission: permission_name,
        });
    }
}

fn inspect_schedule_dependencies(
    nodes: &[AgentScheduleNode],
    issues: &mut Vec<AgentScheduleIssue>,
) {
    let known = nodes
        .iter()
        .map(|node| node.id.as_str())
        .collect::<HashSet<_>>();
    for node in nodes {
        for dep in &node.deps {
            if !known.contains(dep.as_str()) {
                issues.push(AgentScheduleIssue::UnknownDependency {
                    node: node.id.clone(),
                    dependency: dep.clone(),
                });
            }
        }
    }

    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    for node in nodes {
        visit_schedule_node(node.id.as_str(), nodes, &mut visiting, &mut visited, issues);
    }
}

fn visit_schedule_node(
    node: &str,
    nodes: &[AgentScheduleNode],
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
    issues: &mut Vec<AgentScheduleIssue>,
) {
    if visited.contains(node) {
        return;
    }
    if !visiting.insert(node.to_owned()) {
        issues.push(AgentScheduleIssue::DependencyCycle {
            node: node.to_owned(),
        });
        return;
    }
    if let Some(current) = nodes.iter().find(|candidate| candidate.id == node) {
        for dep in &current.deps {
            visit_schedule_node(dep, nodes, visiting, visited, issues);
        }
    }
    visiting.remove(node);
    visited.insert(node.to_owned());
}

fn inspect_completed_nodes(
    nodes: &[AgentScheduleNode],
    completed_nodes: &[&str],
    issues: &mut Vec<AgentScheduleIssue>,
) {
    let known = nodes
        .iter()
        .map(AgentScheduleNode::id)
        .collect::<HashSet<_>>();
    for node in completed_nodes {
        if !is_object_name(node) || !known.contains(*node) {
            issues.push(AgentScheduleIssue::UnknownCompletedNode {
                node: (*node).to_owned(),
            });
        }
    }
}

fn required_object_name(
    node: Option<&String>,
    field: &str,
    value: Option<&Value>,
    issues: &mut Vec<AgentScheduleIssue>,
) -> Option<String> {
    let value = required_word(node, field, value, issues)?;
    if !is_object_name(&value) {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node.cloned(),
            field: field.to_owned(),
            value,
        });
        return None;
    }
    Some(value)
}

fn required_permission_object_name(
    node: Option<&String>,
    field: &str,
    value: Option<&Value>,
    class: PolicyObjectClass,
    issues: &mut Vec<AgentScheduleIssue>,
) -> Option<String> {
    let value = required_word(node, field, value, issues)?;
    let valid = match class {
        PolicyObjectClass::Model => is_model_reference(&value),
        PolicyObjectClass::Tool
        | PolicyObjectClass::Shared
        | PolicyObjectClass::Session
        | PolicyObjectClass::Mount
        | PolicyObjectClass::Agent
        | PolicyObjectClass::Network => is_object_name(&value),
    };
    if !valid {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node.cloned(),
            field: field.to_owned(),
            value,
        });
        return None;
    }
    Some(value)
}

fn required_word(
    node: Option<&String>,
    field: &str,
    value: Option<&Value>,
    issues: &mut Vec<AgentScheduleIssue>,
) -> Option<String> {
    let Some(value) = value else {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node.cloned(),
            field: field.to_owned(),
            value: String::new(),
        });
        return None;
    };
    let Some(value) = value.as_str() else {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node.cloned(),
            field: field.to_owned(),
            value: value.to_string(),
        });
        return None;
    };
    if value.is_empty() || value.bytes().any(|byte| byte.is_ascii_control()) {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node.cloned(),
            field: field.to_owned(),
            value: value.to_owned(),
        });
        return None;
    }
    Some(value.to_owned())
}

fn required_handoff_text(
    node: Option<&String>,
    value: &Value,
    issues: &mut Vec<AgentScheduleIssue>,
) -> Option<String> {
    let Some(value) = value.as_str() else {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node.cloned(),
            field: "handoff".to_owned(),
            value: value.to_string(),
        });
        return None;
    };
    if value.trim().is_empty() || value.contains('\0') {
        issues.push(AgentScheduleIssue::InvalidField {
            node: node.cloned(),
            field: "handoff".to_owned(),
            value: value.to_owned(),
        });
        return None;
    }
    Some(value.to_owned())
}

fn valid_react_bound(value: Option<&Value>) -> bool {
    matches!(value.and_then(Value::as_u64), Some(1..=64))
}
