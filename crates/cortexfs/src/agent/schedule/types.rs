use std::collections::{HashMap, HashSet, VecDeque};

use serde::Deserialize;
use serde_json::Value;

use crate::{
    PolicyObjectClass, PolicyPermission, PolicyV0, abi::path::is_model_reference, is_object_name,
};

/// Maximum number of nodes accepted in one parent-session hybrid schedule.
pub const MAX_AGENT_SCHEDULE_NODES: usize = 1024;

const DEFAULT_DELEGATED_AGENT: &str = "worker";

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
