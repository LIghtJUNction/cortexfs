use super::*;

/// Creates or replaces the parent-owned child handoff channel.
///
/// This writes only the documented `context/child/<child>/` files under the
/// parent session. It does not copy parent `messages.jsonl`, preserving the
/// child-context isolation rule.
pub fn record_child_handoff_to_parent_context(
    parent_session_dir: &Path,
    child_name: &str,
    child_agent: &str,
    child_session: &str,
    handoff: &str,
) -> Result<(), ChildContextRecordError> {
    validate_child_context_names(child_name, child_agent, child_session)?;
    if handoff.contains('\0') {
        return Err(ChildContextRecordError::InvalidText);
    }
    require_parent_session_context(parent_session_dir)?;

    let child_dir = parent_session_dir.join("context/child").join(child_name);
    create_private_context_dir(&child_dir)
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    create_private_context_dir(&child_dir.join("artifact"))
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    for (file, value) in [
        ("agent", child_agent),
        ("session", child_session),
        ("status", ChildContextStatus::Pending.as_str()),
    ] {
        write_child_context_file(&child_dir, file, &format!("{value}\n"))?;
    }
    write_child_context_file(&child_dir, "handoff.md", &ensure_trailing_newline(handoff))?;
    write_text_file_if_absent(&child_dir.join("result.md"), "")
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    write_text_file_if_absent(&child_dir.join("refs.jsonl"), "")
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;

    Ok(())
}

/// Records a child result back into the parent session's child channel.
///
/// The result and refs are inspectable from the parent context pack through
/// `context/child/<child>/result.md` and `refs.jsonl`. This helper keeps the
/// child's full durable history in the child session, not in the parent pack.
pub fn record_child_result_to_parent_context(
    parent_session_dir: &Path,
    child_name: &str,
    status: ChildContextStatus,
    result: &str,
    refs_jsonl: &str,
) -> Result<(), ChildContextRecordError> {
    if !is_object_name(child_name) {
        return Err(ChildContextRecordError::InvalidChildName);
    }
    if matches!(
        status,
        ChildContextStatus::Pending | ChildContextStatus::Active
    ) {
        return Err(ChildContextRecordError::InvalidStatus);
    }
    if result.contains('\0') || refs_jsonl.contains('\0') {
        return Err(ChildContextRecordError::InvalidText);
    }
    if !inspect_context_jsonl(ContextJsonlKind::Refs, refs_jsonl).is_ok() {
        return Err(ChildContextRecordError::InvalidRefs);
    }

    let child_dir = parent_session_dir.join("context/child").join(child_name);
    require_child_context_files(&child_dir)?;
    write_child_context_file(&child_dir, "status", &format!("{}\n", status.as_str()))?;
    write_child_context_file(&child_dir, "result.md", &ensure_trailing_newline(result))?;
    write_child_context_file(
        &child_dir,
        "refs.jsonl",
        &ensure_trailing_newline(refs_jsonl),
    )?;

    Ok(())
}

/// Validates and records a parent-owned hybrid DAG/ReAct schedule.
///
/// The schedule is ordinary parent session context at `context/plan.json`.
/// Recording it does not create agents, enqueue jobs, start a watcher, or grant
/// authority; every declared `requires` entry must already be allowed by the
/// parent effective policy.
pub fn record_agent_schedule_to_parent_context(
    parent_session_dir: &Path,
    schedule_json: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
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

/// Records ready delegated schedule nodes into parent child handoff channels.
///
/// This materializes only parent-owned handoff files for ready nodes that
/// declare `child` and `handoff` in `context/plan.json`. It does not create or
/// start child agents and does not mark schedule nodes complete.
pub fn record_ready_agent_schedule_child_handoffs_to_parent_context(
    parent_session_dir: &Path,
    schedule_json: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
    completed_nodes: &[&str],
) -> Result<Vec<AgentScheduleChildHandoff>, AgentScheduleRecordError> {
    let default_child_session = parent_session_name(parent_session_dir)?;
    let handoffs = ready_agent_schedule_child_handoffs(
        schedule_json,
        parent_subject,
        parent_policy,
        completed_nodes,
        &default_child_session,
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

/// Derives completed hybrid schedule nodes from durable parent-visible state.
///
/// Local parent-owned node completions are supplied explicitly. Delegated nodes
/// are complete when their `context/child/<child>/status` file is a plain file
/// containing `done`.
pub fn completed_agent_schedule_nodes_from_parent_context(
    parent_session_dir: &Path,
    schedule_json: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
    local_completed_nodes: &[&str],
) -> Result<Vec<String>, AgentScheduleRecordError> {
    let nodes = agent_schedule_nodes(schedule_json, parent_subject, parent_policy)
        .map_err(AgentScheduleRecordError::InvalidSchedule)?;
    require_agent_schedule_parent_context(parent_session_dir)?;

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

    let default_child_session = parent_session_name(parent_session_dir)?;
    for node in nodes {
        let Some(child) = node.child() else {
            continue;
        };
        let child_dir = parent_session_dir.join("context").join("child").join(child);
        let Some(handoff) = node.handoff() else {
            return Err(AgentScheduleRecordError::CannotRecord);
        };
        let child_session = node.child_session().unwrap_or(&default_child_session);
        if !schedule_child_context_matches(
            parent_session_dir,
            child,
            node.agent(),
            child_session,
            handoff,
        )? {
            continue;
        }
        match read_child_schedule_status(&child_dir)? {
            Some(ChildContextStatus::Done) if seen.insert(node.id().to_owned()) => {
                completed.push(node.id().to_owned());
            }
            Some(_) | None => {}
        }
    }

    Ok(completed)
}

pub(crate) fn parent_session_name(
    parent_session_dir: &Path,
) -> Result<String, AgentScheduleRecordError> {
    let Some(name) = parent_session_dir
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return Err(AgentScheduleRecordError::MissingParentSession);
    };
    if !is_object_name(name) {
        return Err(AgentScheduleRecordError::MissingParentSession);
    }
    Ok(name.to_owned())
}

/// Advances a parent-owned hybrid schedule from durable parent context.
///
/// This reads delegated child statuses, combines them with explicit local
/// completions, and materializes ready delegated handoffs. It is a single
/// parent-session state transition helper, not a scheduler loop.
pub fn advance_agent_schedule_from_parent_context(
    parent_session_dir: &Path,
    schedule_json: &str,
    parent_subject: &str,
    parent_policy: &PolicyV0,
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

/// Derives the stable `session/index/by-cwd/<key>` file name for a chroot cwd.
#[must_use]
pub fn session_index_key_for_cwd(cwd: &str) -> Option<String> {
    if !is_stable_chroot_absolute_path(cwd) {
        return None;
    }
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in cwd.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Some(format!("cwd-{hash:016x}"))
}

pub(crate) fn record_socket_send_to_session(
    session_dir: &Path,
    id: &str,
    session: &str,
    scope: SocketSessionScope,
    cwd: Option<&str>,
    input: &str,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    if scope == SocketSessionScope::Temp {
        return Err(SocketSessionRecordError::TempSessionNotDurable);
    }
    validate_socket_object_field("id", id)
        .map_err(|_error| SocketSessionRecordError::InvalidField("id"))?;
    if input.contains('\0') {
        return Err(SocketSessionRecordError::InvalidField("input"));
    }
    require_socket_session_name(session_dir, session)?;
    require_socket_session_files(session_dir)?;

    let message = serde_json::json!({
        "role": "user",
        "content": input
    })
    .to_string();
    let event = serde_json::json!({
        "type": "start",
        "id": id,
        "run": id
    })
    .to_string();

    append_session_lines(session_dir, "messages.jsonl", &[&message])?;
    append_session_lines(session_dir, "events.jsonl", &[&event])?;
    set_session_state(session_dir, "active")?;
    if let Some(cwd) = cwd {
        write_session_file(session_dir, "cwd", &format!("{cwd}\n"))?;
    }

    Ok(SocketSessionRecord::new(vec![message], vec![event]))
}

pub(crate) fn record_socket_cancel_to_session(
    session_dir: &Path,
    run_id: &str,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    validate_socket_object_field("id", run_id)
        .map_err(|_error| SocketSessionRecordError::InvalidField("id"))?;
    require_socket_session_files(session_dir)?;

    let event = done_event_json(run_id, "cancelled");
    append_session_lines(session_dir, "events.jsonl", &[&event])?;
    set_session_state(session_dir, "cancelled")?;

    Ok(SocketSessionRecord::new(Vec::new(), vec![event]))
}

pub(crate) fn text_content_parts(content: &str) -> Value {
    serde_json::json!([{ "type": "text", "text": content }])
}

pub(crate) fn done_event_json(run_id: &str, status: &str) -> String {
    serde_json::json!({ "type": "done", "run": run_id, "status": status }).to_string()
}

pub(crate) fn validate_child_context_names(
    child_name: &str,
    child_agent: &str,
    child_session: &str,
) -> Result<(), ChildContextRecordError> {
    for (value, error) in [
        (child_name, ChildContextRecordError::InvalidChildName),
        (child_agent, ChildContextRecordError::InvalidAgentName),
        (child_session, ChildContextRecordError::InvalidSessionName),
    ] {
        if !is_object_name(value) {
            return Err(error);
        }
    }
    Ok(())
}
