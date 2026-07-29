use std::path::Path;

use crate::{
    AgentScheduleChildHandoff, AgentScheduleRecordError, PolicyEvaluator, atomic_replace_text,
    ensure_trailing_newline, inspect_agent_schedule_json,
    ready_agent_schedule_child_handoffs, record_child_handoff_to_parent_context,
};
use crate::runtime::record::{
    agent_schedule_child_record_error, require_agent_schedule_parent_context,
    schedule_child_handoff_materialized,
};

use super::parent_session_name;

/// Validates and records an ordinary parent-session hybrid schedule.
pub fn record_agent_schedule_to_parent_context(
    parent_session_dir: &Path,
    schedule_json: &str,
    parent_subject: &str,
    parent_policy: &dyn PolicyEvaluator,
) -> Result<(), AgentScheduleRecordError> {
    if schedule_json.contains('\0') {
        return Err(AgentScheduleRecordError::InvalidText);
    }
    let report = inspect_agent_schedule_json(schedule_json, parent_subject, parent_policy);
    if !report.is_ok() {
        return Err(AgentScheduleRecordError::InvalidSchedule(report));
    }
    require_agent_schedule_parent_context(parent_session_dir)?;
    atomic_replace_text(
        &parent_session_dir.join("context").join("plan.json"),
        &ensure_trailing_newline(schedule_json),
    )
    .map_err(|_error| AgentScheduleRecordError::CannotRecord)
}

/// Materializes ready delegated nodes as parent-owned child handoffs.
pub fn record_ready_agent_schedule_child_handoffs_to_parent_context(
    parent_session_dir: &Path,
    schedule_json: &str,
    parent_subject: &str,
    parent_policy: &dyn PolicyEvaluator,
    completed_nodes: &[&str],
) -> Result<Vec<AgentScheduleChildHandoff>, AgentScheduleRecordError> {
    let session = parent_session_name(parent_session_dir)?;
    let handoffs = ready_agent_schedule_child_handoffs(
        schedule_json,
        parent_subject,
        parent_policy,
        completed_nodes,
        &session,
    )
    .map_err(AgentScheduleRecordError::InvalidSchedule)?;
    require_agent_schedule_parent_context(parent_session_dir)?;

    let mut recorded = Vec::new();
    for handoff in handoffs {
        if schedule_child_handoff_materialized(parent_session_dir, &handoff)? {
            continue;
        }
        record_child_handoff_to_parent_context(
            parent_session_dir,
            handoff.child(),
            handoff.agent(),
            handoff.session(),
            handoff.handoff(),
        )
        .map_err(agent_schedule_child_record_error)?;
        recorded.push(handoff);
    }
    Ok(recorded)
}
