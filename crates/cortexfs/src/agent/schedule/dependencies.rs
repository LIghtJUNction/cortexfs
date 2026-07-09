use super::*;
use crate::abi::path::is_model_reference;
use crate::*;
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) fn inspect_schedule_dependencies(
    nodes: &[AgentScheduleNode],
    issues: &mut Vec<AgentScheduleIssue>,
) {
    let known = nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str(), index))
        .collect::<HashMap<_, _>>();
    let mut dependents = vec![Vec::new(); nodes.len()];
    let mut pending_deps = vec![0_usize; nodes.len()];
    for (node_index, node) in nodes.iter().enumerate() {
        for dep in &node.deps {
            if let Some(dep_index) = known.get(dep.as_str()).copied() {
                if let Some(dep_dependents) = dependents.get_mut(dep_index) {
                    dep_dependents.push(node_index);
                }
                if let Some(node_pending_deps) = pending_deps.get_mut(node_index) {
                    *node_pending_deps += 1;
                }
            } else {
                issues.push(AgentScheduleIssue::UnknownDependency {
                    node: node.id.clone(),
                    dependency: dep.clone(),
                });
            }
        }
    }

    let mut ready = pending_deps
        .iter()
        .enumerate()
        .filter_map(|(index, count)| (*count == 0).then_some(index))
        .collect::<VecDeque<_>>();
    let mut visited = 0_usize;
    while let Some(node_index) = ready.pop_front() {
        visited += 1;
        let Some(node_dependents) = dependents.get(node_index) else {
            continue;
        };
        for dependent_index in node_dependents {
            let Some(dependent_pending_deps) = pending_deps.get_mut(*dependent_index) else {
                continue;
            };
            debug_assert!(
                *dependent_pending_deps > 0,
                "agent schedule dependency counts stay consistent"
            );
            if *dependent_pending_deps == 0 {
                continue;
            }
            *dependent_pending_deps -= 1;
            if *dependent_pending_deps == 0 {
                ready.push_back(*dependent_index);
            }
        }
    }

    if visited != nodes.len()
        && let Some(node) = nodes.iter().enumerate().find_map(|(index, node)| {
            pending_deps
                .get(index)
                .is_some_and(|pending| *pending > 0)
                .then_some(node)
        })
    {
        issues.push(AgentScheduleIssue::DependencyCycle {
            node: node.id.clone(),
        });
    }
}

pub(crate) fn inspect_completed_nodes(
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

pub(crate) fn required_object_name(
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

pub(crate) fn required_permission_object_name(
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

pub(crate) fn required_word(
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

pub(crate) fn required_handoff_text(
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

pub(crate) fn valid_react_bound(value: Option<&Value>) -> bool {
    matches!(value.and_then(Value::as_u64), Some(1..=64))
}
