use std::collections::HashSet;
use std::path::Path;

use crate::runtime::record::{
    agent_schedule_child_record_error, read_child_schedule_status, require_parent_session_context,
    schedule_child_context_matches,
};
use crate::{
    AgentScheduleIssue, AgentScheduleNode, AgentScheduleRecordError, AgentScheduleReport,
    ChildContextStatus, PolicyEvaluator, agent_schedule_nodes, is_object_name,
};

/// Derives completed nodes from explicit local results and child state.
pub fn completed_agent_schedule_nodes_from_parent_context(
    parent_session_dir: &Path,
    schedule_json: &str,
    parent_subject: &str,
    parent_policy: &dyn PolicyEvaluator,
    local_completed_nodes: &[&str],
) -> Result<Vec<String>, AgentScheduleRecordError> {
    let nodes = agent_schedule_nodes(schedule_json, parent_subject, parent_policy)
        .map_err(AgentScheduleRecordError::InvalidSchedule)?;
    require_parent_session_context(parent_session_dir)
        .map_err(agent_schedule_child_record_error)?;

    let known = nodes
        .iter()
        .map(AgentScheduleNode::id)
        .collect::<HashSet<_>>();
    let mut completed = Vec::new();
    let mut seen = HashSet::new();
    let mut issues = Vec::new();
    for node in local_completed_nodes {
        if !is_object_name(node) || !known.contains(*node) {
            issues.push(AgentScheduleIssue::UnknownCompletedNode {
                node: (*node).to_owned(),
            });
        } else if nodes
            .iter()
            .any(|candidate| candidate.id() == *node && candidate.child().is_some())
        {
            issues.push(AgentScheduleIssue::DelegatedCompletionRequiresChildResult {
                node: (*node).to_owned(),
            });
        } else if seen.insert((*node).to_owned()) {
            completed.push((*node).to_owned());
        }
    }
    if !issues.is_empty() {
        return Err(AgentScheduleRecordError::InvalidSchedule(
            AgentScheduleReport::new(issues),
        ));
    }

    let session = parent_session_name(parent_session_dir)?;
    for node in nodes {
        let Some(child) = node.child() else {
            continue;
        };
        let child_dir = parent_session_dir.join("context/child").join(child);
        let Some(handoff) = node.handoff() else {
            return Err(AgentScheduleRecordError::CannotRecord);
        };
        let child_session = node.child_session().unwrap_or(&session);
        if !schedule_child_context_matches(
            parent_session_dir,
            child,
            node.agent(),
            child_session,
            handoff,
        )? {
            continue;
        }
        if matches!(
            read_child_schedule_status(&child_dir)?,
            Some(ChildContextStatus::Done)
        ) && seen.insert(node.id().to_owned())
        {
            completed.push(node.id().to_owned());
        }
    }
    Ok(completed)
}

pub fn parent_session_name(parent_session_dir: &Path) -> Result<String, AgentScheduleRecordError> {
    let name = parent_session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| is_object_name(name))
        .ok_or(AgentScheduleRecordError::MissingParentSession)?;
    Ok(name.to_owned())
}
