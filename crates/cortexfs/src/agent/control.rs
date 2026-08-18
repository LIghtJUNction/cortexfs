use std::collections::BTreeSet;

use crate::{
    AgentPermissions, ChildLifecycle, ControlLineIssue,
    abi::path::is_object_name,
    authority::parent_ref_agent_name,
    support::control::{
        inspect_control_line, inspect_control_lines, parse_canonical_control_value,
        parse_canonical_positive_u32,
    },
};

/// Stable agent control file kind with fixed value syntax.
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
    /// `agent/<name>.d/perm`: coarse read, write, and shell permissions.
    Perm,
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
    /// `agent/<name>.d/approval`: hosted direct-native approval mode.
    Approval,
    /// `agent/<name>.d/window`: context-window setting in tokens.
    Window,
}

impl AgentControlKind {
    /// Parses an agent control file name with fixed syntax.
    #[must_use]
    pub fn parse(file_name: &str) -> Option<Self> {
        match file_name {
            "owner" => Some(Self::Owner),
            "uid" => Some(Self::Uid),
            "gid" => Some(Self::Gid),
            "groups" => Some(Self::Groups),
            "perm" => Some(Self::Perm),
            "iso" => Some(Self::Iso),
            "parent" => Some(Self::Parent),
            "life" => Some(Self::Life),
            "status" => Some(Self::Status),
            "pid" => Some(Self::Pid),
            "approval" => Some(Self::Approval),
            "window" => Some(Self::Window),
            _ => None,
        }
    }
}

/// Agent control-file validation uses the shared control-line issue model.
pub type AgentControlIssue = ControlLineIssue;

/// Result of inspecting a fixed-format agent control file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentControlReport {
    issues: Vec<ControlLineIssue>,
}

impl_issue_report!(AgentControlReport, ControlLineIssue);

/// Inspects a fixed-format agent control file body.
#[must_use]
pub fn inspect_agent_control(kind: AgentControlKind, content: &str) -> AgentControlReport {
    match kind {
        AgentControlKind::Groups => inspect_agent_groups_control(content),
        AgentControlKind::Parent => inspect_optional_agent_parent_control(content),
        AgentControlKind::Pid => inspect_optional_agent_number_control(content),
        AgentControlKind::Window => inspect_agent_window_control(content),
        AgentControlKind::Perm => AgentControlReport::new(
            AgentPermissions::parse_control(content)
                .is_none()
                .then(|| ControlLineIssue::InvalidValue {
                    line: 1,
                    value: content.trim_end().to_owned(),
                })
                .into_iter()
                .collect(),
        ),
        AgentControlKind::Owner | AgentControlKind::Uid | AgentControlKind::Gid => {
            inspect_required_agent_number_control(content)
        }
        AgentControlKind::Iso
        | AgentControlKind::Life
        | AgentControlKind::Status
        | AgentControlKind::Approval => inspect_agent_vocab_control(kind, content),
    }
}

pub(crate) fn inspect_agent_window_control(content: &str) -> AgentControlReport {
    let mut issues = inspect_control_line(content, true, |line, value, issues| {
        if value != "auto" && parse_canonical_positive_u32(value).is_none() {
            issues.push(ControlLineIssue::InvalidNumber {
                line,
                value: value.to_owned(),
            });
        }
    });
    if parse_canonical_control_value(content).is_none() && issues.is_empty() {
        issues.push(ControlLineIssue::InvalidValue {
            line: 1,
            value: content.to_owned(),
        });
    }
    AgentControlReport::new(issues)
}

/// Inspects the optional direct-native tool declaration control.
#[must_use]
pub fn inspect_agent_tools_control(content: &str) -> AgentControlReport {
    if content.is_empty() || content == "\n" {
        return AgentControlReport::default();
    }
    let mut seen = BTreeSet::new();
    let mut issues = inspect_control_lines(content, |line, value, issues| {
        if value == "tsh" || !is_object_name(value) || !seen.insert(value.to_owned()) {
            issues.push(ControlLineIssue::InvalidValue {
                line,
                value: value.to_owned(),
            });
        }
    });
    if !content.is_empty() && !content.ends_with('\n') {
        issues.push(ControlLineIssue::InvalidValue {
            line: content.lines().count().max(1),
            value: content.lines().last().unwrap_or_default().to_owned(),
        });
    }
    AgentControlReport::new(issues)
}

pub(crate) fn inspect_required_agent_number_control(content: &str) -> AgentControlReport {
    AgentControlReport::new(inspect_control_line(
        content,
        true,
        |line, value, issues| {
            if value.parse::<u32>().is_err() {
                issues.push(ControlLineIssue::InvalidNumber {
                    line,
                    value: value.to_owned(),
                });
            }
        },
    ))
}

pub(crate) fn inspect_optional_agent_number_control(content: &str) -> AgentControlReport {
    AgentControlReport::new(inspect_control_line(
        content,
        false,
        |line, value, issues| {
            if !value.is_empty() && value.parse::<u32>().is_err() {
                issues.push(ControlLineIssue::InvalidNumber {
                    line,
                    value: value.to_owned(),
                });
            }
        },
    ))
}

pub(crate) fn inspect_agent_groups_control(content: &str) -> AgentControlReport {
    AgentControlReport::new(inspect_control_lines(content, |line, value, issues| {
        if value.parse::<u32>().is_err() {
            issues.push(ControlLineIssue::InvalidNumber {
                line,
                value: value.to_owned(),
            });
        }
    }))
}

pub(crate) fn inspect_optional_agent_parent_control(content: &str) -> AgentControlReport {
    AgentControlReport::new(inspect_control_line(
        content,
        false,
        |line, value, issues| {
            if !value.is_empty() && parent_ref_agent_name(value).is_err() {
                issues.push(ControlLineIssue::InvalidValue {
                    line,
                    value: value.to_owned(),
                });
            }
        },
    ))
}

pub(crate) fn inspect_agent_vocab_control(
    kind: AgentControlKind,
    content: &str,
) -> AgentControlReport {
    AgentControlReport::new(inspect_control_line(
        content,
        true,
        |line, value, issues| {
            if !agent_vocab_allows(kind, value) {
                issues.push(ControlLineIssue::InvalidValue {
                    line,
                    value: value.to_owned(),
                });
            }
        },
    ))
}

pub(crate) fn agent_vocab_allows(kind: AgentControlKind, value: &str) -> bool {
    match kind {
        AgentControlKind::Iso => matches!(value, "shared" | "uid" | "userns"),
        AgentControlKind::Life => ChildLifecycle::parse(value).is_ok(),
        AgentControlKind::Status => {
            matches!(
                value,
                "start" | "ready" | "busy" | "idle" | "stopping" | "dead"
            )
        }
        AgentControlKind::Approval => matches!(value, "auto" | "ask"),
        AgentControlKind::Owner
        | AgentControlKind::Uid
        | AgentControlKind::Gid
        | AgentControlKind::Groups
        | AgentControlKind::Parent
        | AgentControlKind::Perm
        | AgentControlKind::Pid
        | AgentControlKind::Window => false,
    }
}
