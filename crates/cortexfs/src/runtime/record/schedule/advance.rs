use std::path::Path;

use crate::{AgentScheduleAdvance, AgentScheduleRecordError, PolicyEvaluator};

use super::{
    completed_agent_schedule_nodes_from_parent_context,
    record_ready_agent_schedule_child_handoffs_to_parent_context,
};

/// Advances one parent-session state transition without a scheduler loop.
pub fn advance_agent_schedule_from_parent_context(
    parent_session_dir: &Path,
    schedule_json: &str,
    parent_subject: &str,
    parent_policy: &dyn PolicyEvaluator,
    local_completed_nodes: &[&str],
) -> Result<AgentScheduleAdvance, AgentScheduleRecordError> {
    let completed_nodes = completed_agent_schedule_nodes_from_parent_context(
        parent_session_dir,
        schedule_json,
        parent_subject,
        parent_policy,
        local_completed_nodes,
    )?;
    let completed_refs = completed_nodes
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    let handoffs = record_ready_agent_schedule_child_handoffs_to_parent_context(
        parent_session_dir,
        schedule_json,
        parent_subject,
        parent_policy,
        &completed_refs,
    )?;
    Ok(AgentScheduleAdvance::new(completed_nodes, handoffs))
}
