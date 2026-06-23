use crate::{ChildLifecycle, parent_ref_agent_name};

/// Stable agent control file kind with fixed v1 value syntax.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentControlKind {
    /// `agent/<name>.d/owner`: owning Linux uid.
    Owner,
    /// `agent/<name>.d/uid`: runtime Linux uid.
    Uid,
    /// `agent/<name>.d/gid`: runtime Linux gid.
    Gid,
    /// `agent/<name>.d/groups`: supplementary groups, one gid per line.
    Groups,
    /// `agent/<name>.d/iso`: isolation profile.
    Iso,
    /// `agent/<name>.d/parent`: parent agent/session/run reference.
    Parent,
    /// `agent/<name>.d/life`: lifecycle ownership.
    Life,
    /// `agent/<name>.d/status`: process lifecycle state.
    Status,
    /// `agent/<name>.d/pid`: runtime process id, when running.
    Pid,
}

impl AgentControlKind {
    /// Parses an agent control file name with fixed v1 syntax.
    #[must_use]
    pub fn parse(file_name: &str) -> Option<Self> {
        match file_name {
            "owner" => Some(Self::Owner),
            "uid" => Some(Self::Uid),
            "gid" => Some(Self::Gid),
            "groups" => Some(Self::Groups),
            "iso" => Some(Self::Iso),
            "parent" => Some(Self::Parent),
            "life" => Some(Self::Life),
            "status" => Some(Self::Status),
            "pid" => Some(Self::Pid),
            _ => None,
        }
    }
}

/// Agent control-file validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentControlIssue {
    /// A required single value is empty.
    EmptyValue,
    /// A single-value control file contains more than one line.
    MultipleValues { line: usize },
    /// Numeric uid/gid/pid value is malformed.
    InvalidNumber { line: usize, value: String },
    /// Fixed vocabulary or parent reference value is malformed.
    InvalidValue { line: usize, value: String },
}

/// Result of inspecting a fixed-format agent control file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentControlReport {
    issues: Vec<AgentControlIssue>,
}

impl_issue_report!(AgentControlReport, AgentControlIssue);

/// Inspects a fixed-format v1 agent control file body.
#[must_use]
pub fn inspect_agent_control(kind: AgentControlKind, content: &str) -> AgentControlReport {
    match kind {
        AgentControlKind::Groups => inspect_agent_groups_control(content),
        AgentControlKind::Parent => inspect_optional_agent_parent_control(content),
        AgentControlKind::Pid => inspect_optional_agent_number_control(content),
        AgentControlKind::Owner | AgentControlKind::Uid | AgentControlKind::Gid => {
            inspect_required_agent_number_control(content)
        }
        AgentControlKind::Iso | AgentControlKind::Life | AgentControlKind::Status => {
            inspect_agent_vocab_control(kind, content)
        }
    }
}

fn inspect_required_agent_number_control(content: &str) -> AgentControlReport {
    inspect_single_agent_control_value(content, true, |line, value, issues| {
        if value.parse::<u32>().is_err() {
            issues.push(AgentControlIssue::InvalidNumber {
                line,
                value: value.to_owned(),
            });
        }
    })
}

fn inspect_optional_agent_number_control(content: &str) -> AgentControlReport {
    inspect_single_agent_control_value(content, false, |line, value, issues| {
        if !value.is_empty() && value.parse::<u32>().is_err() {
            issues.push(AgentControlIssue::InvalidNumber {
                line,
                value: value.to_owned(),
            });
        }
    })
}

fn inspect_agent_groups_control(content: &str) -> AgentControlReport {
    let mut issues = Vec::new();
    for (index, raw_line) in content.lines().enumerate() {
        let line = index + 1;
        let value = raw_line.trim();
        if value.is_empty() {
            issues.push(AgentControlIssue::EmptyValue);
        } else if value != raw_line || value.parse::<u32>().is_err() {
            issues.push(AgentControlIssue::InvalidNumber {
                line,
                value: value.to_owned(),
            });
        }
    }
    AgentControlReport::new(issues)
}

fn inspect_optional_agent_parent_control(content: &str) -> AgentControlReport {
    inspect_single_agent_control_value(content, false, |line, value, issues| {
        if !value.is_empty() && parent_ref_agent_name(value).is_err() {
            issues.push(AgentControlIssue::InvalidValue {
                line,
                value: value.to_owned(),
            });
        }
    })
}

fn inspect_agent_vocab_control(kind: AgentControlKind, content: &str) -> AgentControlReport {
    inspect_single_agent_control_value(content, true, |line, value, issues| {
        if !agent_vocab_allows(kind, value) {
            issues.push(AgentControlIssue::InvalidValue {
                line,
                value: value.to_owned(),
            });
        }
    })
}

fn inspect_single_agent_control_value(
    content: &str,
    required: bool,
    validate: impl Fn(usize, &str, &mut Vec<AgentControlIssue>),
) -> AgentControlReport {
    let mut issues = Vec::new();
    let lines = content.lines().collect::<Vec<_>>();
    let value = lines.first().map_or("", |line| line.trim());
    if value.is_empty() {
        if required {
            issues.push(AgentControlIssue::EmptyValue);
        }
    } else if lines.first().is_some_and(|line| *line != value) {
        issues.push(AgentControlIssue::InvalidValue {
            line: 1,
            value: value.to_owned(),
        });
    } else {
        validate(1, value, &mut issues);
    }
    if lines.len() > 1 {
        issues.push(AgentControlIssue::MultipleValues { line: 2 });
    }
    AgentControlReport::new(issues)
}

fn agent_vocab_allows(kind: AgentControlKind, value: &str) -> bool {
    match kind {
        AgentControlKind::Iso => matches!(value, "shared" | "uid" | "userns"),
        AgentControlKind::Life => ChildLifecycle::parse(value).is_ok(),
        AgentControlKind::Status => {
            matches!(
                value,
                "start" | "ready" | "busy" | "idle" | "stopping" | "dead"
            )
        }
        AgentControlKind::Owner
        | AgentControlKind::Uid
        | AgentControlKind::Gid
        | AgentControlKind::Groups
        | AgentControlKind::Parent
        | AgentControlKind::Pid => false,
    }
}
