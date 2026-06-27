type SocketRecordResult<T> = Result<T, SocketSessionRecordError>;

/// Records durable filesystem effects for a parsed socket request.
///
/// `send` appends a user message to `messages.jsonl`, appends a canonical
/// `start` event to `events.jsonl`, marks the session active, and records a
/// supplied chroot `cwd` when present. `cancel` appends a cancelled `done`
/// event and marks the session cancelled. `resume`, `ping`, and temp sessions
/// do not mutate durable session files.
pub fn record_socket_request_to_session(
    session_dir: &Path,
    request: &SocketRequest,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    match *request {
        SocketRequest::Send {
            ref id,
            ref session,
            scope,
            ref cwd,
            ref input,
        } => record_socket_send_to_session(session_dir, id, session, scope, cwd.as_deref(), input),
        SocketRequest::Cancel { ref id } => record_socket_cancel_to_session(session_dir, id),
        SocketRequest::Resume { .. } | SocketRequest::Ping => {
            Err(SocketSessionRecordError::UnsupportedRequest)
        }
    }
}

/// Records a durable socket `send` under `session_root/<session>/` and updates
/// the reserved session index files.
///
/// This is a filesystem helper for socket runtimes. It does not create
/// sessions, start models, or interpret provider state. The selected session
/// must already exist and have the v1 durable files.
pub fn record_indexed_socket_send_to_session(
    session_root: &Path,
    request: &SocketRequest,
) -> Result<SocketSessionRecord, IndexedSocketSessionRecordError> {
    let (session, scope, cwd) = match *request {
        SocketRequest::Send {
            ref session,
            scope,
            ref cwd,
            ..
        } => (session.as_str(), scope, cwd.as_deref()),
        SocketRequest::Resume { .. } | SocketRequest::Cancel { .. } | SocketRequest::Ping => {
            return Err(IndexedSocketSessionRecordError::Session(
                SocketSessionRecordError::UnsupportedRequest,
            ));
        }
    };
    if scope == SocketSessionScope::Temp {
        return Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::TempSessionNotDurable,
        ));
    }

    let by_cwd_key = cwd.and_then(session_index_key_for_cwd);
    preflight_session_index_update(
        session_root,
        session,
        by_cwd_key.as_deref(),
        None,
        None,
    )
    .map_err(IndexedSocketSessionRecordError::Index)?;

    let session_dir = session_root.join(session);
    let record = record_socket_request_to_session(&session_dir, request)
        .map_err(IndexedSocketSessionRecordError::Session)?;
    update_session_index(session_root, session, by_cwd_key.as_deref())
        .map_err(IndexedSocketSessionRecordError::Index)?;

    Ok(record)
}

/// Records a completed assistant response into durable session files.
///
/// This appends an assistant message to `messages.jsonl`, appends canonical
/// `message` and `done` events to `events.jsonl`, writes `latest.md`, and marks
/// the session `done`. Raw history remains append-only; `latest.md` is only the
/// latest inspectable convenience file.
pub fn record_assistant_response_to_session(
    session_dir: &Path,
    run_id: &str,
    content: &str,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    validate_socket_object_field("run", run_id)
        .map_err(|_error| SocketSessionRecordError::SessionMismatch)?;
    if content.contains('\0') {
        return Err(SocketSessionRecordError::InvalidField("content"));
    }
    require_socket_session_files(session_dir)?;

    let content_parts = text_content_parts(content);
    let message = serde_json::json!({
        "role": "assistant",
        "content": content_parts
    })
    .to_string();
    let event = serde_json::json!({
        "type": "message",
        "run": run_id,
        "role": "assistant",
        "content": content_parts
    })
    .to_string();
    let done = done_event_json(run_id, "ok");

    append_session_lines(session_dir, "messages.jsonl", &[&message])?;
    append_session_lines(session_dir, "events.jsonl", &[&event, &done])?;
    write_session_file(session_dir, "latest.md", &format!("{content}\n"))?;
    set_session_state(session_dir, "done")?;

    Ok(SocketSessionRecord::new(vec![message], vec![event, done]))
}

/// Records a denied tool execution as durable session runtime history.
///
/// Denials are facts, not prompt text. Recording them in `events.jsonl` makes
/// policy failures inspectable without granting authority or executing the
/// requested tool.
pub fn record_tool_execution_denial_to_session(
    session_dir: &Path,
    run_id: &str,
    tool_name: &str,
    denial: ToolExecutionDenial,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    validate_socket_object_field("run", run_id)
        .map_err(|_error| SocketSessionRecordError::InvalidField("run"))?;
    if !is_object_name(tool_name) {
        return Err(SocketSessionRecordError::InvalidField("tool"));
    }
    require_socket_session_files(session_dir)?;

    let event = serde_json::json!({
        "type": "error",
        "run": run_id,
        "tool": tool_name,
        "code": denial.errno(),
        "message": "tool execution denied"
    })
    .to_string();
    let done = done_event_json(run_id, "error");

    append_session_lines(session_dir, "events.jsonl", &[&event, &done])?;
    set_session_state(session_dir, "error")?;

    Ok(SocketSessionRecord::new(Vec::new(), vec![event, done]))
}

/// Records a successful tool execution result into durable session history.
///
/// Tool results are ordinary session messages and canonical `message` events.
/// The helper does not execute a tool and does not grant authority; callers
/// must run [`authorize_tool_execution`] before invoking the capability.
pub fn record_tool_execution_result_to_session(
    session_dir: &Path,
    run_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    content: &str,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    validate_socket_object_field("run", run_id)
        .map_err(|_error| SocketSessionRecordError::InvalidField("run"))?;
    validate_socket_object_field("tool_call_id", tool_call_id)
        .map_err(|_error| SocketSessionRecordError::InvalidField("tool_call_id"))?;
    if !is_object_name(tool_name) {
        return Err(SocketSessionRecordError::InvalidField("tool"));
    }
    if content.contains('\0') {
        return Err(SocketSessionRecordError::InvalidField("content"));
    }
    require_socket_session_files(session_dir)?;

    let content_part = serde_json::json!({
        "type": "tool_result",
        "tool_call_id": tool_call_id,
        "content": content
    });
    let message = serde_json::json!({
        "role": "tool",
        "name": tool_name,
        "content": [content_part]
    })
    .to_string();
    let event = serde_json::json!({
        "type": "message",
        "run": run_id,
        "role": "tool",
        "name": tool_name,
        "content": [content_part]
    })
    .to_string();

    append_session_lines(session_dir, "messages.jsonl", &[&message])?;
    append_session_lines(session_dir, "events.jsonl", &[&event])?;
    touch_session(session_dir)?;

    Ok(SocketSessionRecord::new(vec![message], vec![event]))
}

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
    create_private_context_dir(&child_dir).map_err(|_error| ChildContextRecordError::CannotRecord)?;
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
    write_child_context_file(&child_dir, "refs.jsonl", &ensure_trailing_newline(refs_jsonl))?;

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
    let handoffs = ready_agent_schedule_child_handoffs(
        schedule_json,
        parent_subject,
        parent_policy,
        completed_nodes,
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

    for node in nodes {
        let Some(child) = node.child() else {
            continue;
        };
        let child_dir = parent_session_dir
            .join("context")
            .join("child")
            .join(child);
        let Some(handoff) = node.handoff() else {
            return Err(AgentScheduleRecordError::CannotRecord);
        };
        let Some(child_session) = node.child_session() else {
            return Err(AgentScheduleRecordError::CannotRecord);
        };
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

fn record_socket_send_to_session(
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

fn record_socket_cancel_to_session(
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

fn text_content_parts(content: &str) -> Value {
    serde_json::json!([{ "type": "text", "text": content }])
}

fn done_event_json(run_id: &str, status: &str) -> String {
    serde_json::json!({ "type": "done", "run": run_id, "status": status }).to_string()
}

fn validate_child_context_names(
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

fn require_parent_session_context(
    parent_session_dir: &Path,
) -> Result<(), ChildContextRecordError> {
    for file in SESSION_REQUIRED_FILES {
        if !is_plain_existing_file(&parent_session_dir.join(file)) {
            return Err(ChildContextRecordError::MissingParentSession);
        }
    }
    if !is_plain_existing_dir(&parent_session_dir.join("context")) {
        return Err(ChildContextRecordError::MissingParentSession);
    }
    Ok(())
}

fn require_child_context_files(child_dir: &Path) -> Result<(), ChildContextRecordError> {
    for file in CHILD_RESULT_REQUIRED_FILES {
        if !is_plain_existing_file(&child_dir.join(file)) {
            return Err(ChildContextRecordError::MissingParentSession);
        }
    }
    for dir in CHILD_RESULT_REQUIRED_DIRS {
        if !is_plain_existing_dir(&child_dir.join(dir)) {
            return Err(ChildContextRecordError::MissingParentSession);
        }
    }
    Ok(())
}

fn require_agent_schedule_parent_context(
    parent_session_dir: &Path,
) -> Result<(), AgentScheduleRecordError> {
    for file in SESSION_REQUIRED_FILES {
        if !is_plain_existing_file(&parent_session_dir.join(file)) {
            return Err(AgentScheduleRecordError::MissingParentSession);
        }
    }
    if !is_plain_existing_dir(&parent_session_dir.join("context")) {
        return Err(AgentScheduleRecordError::MissingParentSession);
    }
    Ok(())
}

fn agent_schedule_child_record_error(error: ChildContextRecordError) -> AgentScheduleRecordError {
    match error {
        ChildContextRecordError::MissingParentSession => {
            AgentScheduleRecordError::MissingParentSession
        }
        ChildContextRecordError::CannotRecord
        | ChildContextRecordError::InvalidChildName
        | ChildContextRecordError::InvalidAgentName
        | ChildContextRecordError::InvalidSessionName
        | ChildContextRecordError::InvalidStatus
        | ChildContextRecordError::InvalidText
        | ChildContextRecordError::InvalidRefs => AgentScheduleRecordError::CannotRecord,
    }
}

fn schedule_child_handoff_materialized(
    parent_session_dir: &Path,
    handoff: &AgentScheduleChildHandoff,
) -> Result<bool, AgentScheduleRecordError> {
    schedule_child_context_matches(
        parent_session_dir,
        handoff.child(),
        handoff.agent(),
        handoff.session(),
        handoff.handoff(),
    )
}

fn schedule_child_context_matches(
    parent_session_dir: &Path,
    child_name: &str,
    child_agent: &str,
    child_session: &str,
    handoff: &str,
) -> Result<bool, AgentScheduleRecordError> {
    let child_dir = parent_session_dir
        .join("context")
        .join("child")
        .join(child_name);
    match child_dir.symlink_metadata() {
        Ok(metadata) if metadata.is_dir() => {
            require_agent_schedule_child_context_files(&child_dir)?;
            if read_child_schedule_file(&child_dir, "agent")? != format!("{child_agent}\n")
                || read_child_schedule_file(&child_dir, "session")? != format!("{child_session}\n")
                || read_child_schedule_file(&child_dir, "handoff.md")?
                    != ensure_trailing_newline(handoff)
            {
                return Err(AgentScheduleRecordError::CannotRecord);
            }
            let _status = read_child_schedule_status(&child_dir)?;
            let refs_jsonl = read_child_schedule_file(&child_dir, "refs.jsonl")?;
            if !inspect_context_jsonl(ContextJsonlKind::Refs, &refs_jsonl).is_ok() {
                return Err(AgentScheduleRecordError::CannotRecord);
            }
            Ok(true)
        }
        Ok(_metadata) => Err(AgentScheduleRecordError::CannotRecord),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_error) => Err(AgentScheduleRecordError::CannotRecord),
    }
}

fn require_agent_schedule_child_context_files(
    child_dir: &Path,
) -> Result<(), AgentScheduleRecordError> {
    for file in CHILD_RESULT_REQUIRED_FILES {
        if !is_plain_existing_file(&child_dir.join(file)) {
            return Err(AgentScheduleRecordError::CannotRecord);
        }
    }
    for dir in CHILD_RESULT_REQUIRED_DIRS {
        if !is_plain_existing_dir(&child_dir.join(dir)) {
            return Err(AgentScheduleRecordError::CannotRecord);
        }
    }
    Ok(())
}

fn read_child_schedule_file(
    child_dir: &Path,
    file: &str,
) -> Result<String, AgentScheduleRecordError> {
    fs::read_to_string(child_dir.join(file)).map_err(|_error| AgentScheduleRecordError::CannotRecord)
}

fn read_child_schedule_status(
    child_dir: &Path,
) -> Result<Option<ChildContextStatus>, AgentScheduleRecordError> {
    let child_metadata = match child_dir.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_error) => return Err(AgentScheduleRecordError::CannotRecord),
    };
    if !child_metadata.is_dir() {
        return Err(AgentScheduleRecordError::CannotRecord);
    }
    let status_path = child_dir.join("status");
    let metadata = match status_path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_error) => return Err(AgentScheduleRecordError::CannotRecord),
    };
    if !metadata.is_file() {
        return Err(AgentScheduleRecordError::CannotRecord);
    }
    let status = fs::read_to_string(status_path)
        .map_err(|_error| AgentScheduleRecordError::CannotRecord)?;
    let status = status.trim();
    ChildContextStatus::parse(status).map_or(Err(AgentScheduleRecordError::CannotRecord), |status| {
        Ok(Some(status))
    })
}

fn write_child_context_file(
    child_dir: &Path,
    file: &str,
    content: &str,
) -> Result<(), ChildContextRecordError> {
    atomic_replace_text(&child_dir.join(file), content)
        .map_err(|_error| ChildContextRecordError::CannotRecord)
}

fn append_session_lines(dir: &Path, file: &str, lines: &[&str]) -> SocketRecordResult<()> {
    for line in lines {
        append_jsonl_line(&dir.join(file), line)
            .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    }
    Ok(())
}

fn write_session_file(dir: &Path, file: &str, content: &str) -> SocketRecordResult<()> {
    atomic_replace_text(&dir.join(file), content)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)
}

fn set_session_state(dir: &Path, state: &str) -> SocketRecordResult<()> {
    write_session_file(dir, "state", &format!("{state}\n"))?;
    touch_session(dir)
}

fn touch_session(dir: &Path) -> SocketRecordResult<()> {
    write_session_file(dir, "updated_at", &unix_timestamp_text())
}

fn write_text_file_if_absent(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "text file must have a parent",
        )
    })?;
    let name = path_file_name(path).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "text file must have a name")
    })?;
    let parent_dir = open_plain_directory(parent)?;
    match nix::fcntl::openat(
        &parent_dir,
        name,
        nix::fcntl::OFlag::O_RDWR | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    ) {
        Ok(file_fd) => {
            let file = fs::File::from(file_fd);
            if !file.metadata()?.is_file() {
                return Err(std::io::Error::other("path is not a regular file"));
            }
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
            file.sync_all()?;
            parent_dir.sync_all()?;
            return Ok(());
        }
        Err(nix::errno::Errno::ENOENT) => {}
        Err(error) => return Err(std::io::Error::from(error)),
    }
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        name,
        nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_EXCL
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    )
    .map_err(std::io::Error::from)?;
    let mut file = fs::File::from(file_fd);
    file.write_all(content.as_bytes())?;
    file.flush()?;
    file.sync_all()?;
    parent_dir.sync_all()?;
    Ok(())
}

fn create_private_context_dir(path: &Path) -> std::io::Result<()> {
    match open_private_context_dir(path) {
        Ok(dir) => {
            dir.set_permissions(fs::Permissions::from_mode(0o700))?;
            dir.sync_all()
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "context directory must have a parent",
                )
            })?;
            let name = path_file_name(path).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "context directory must have a file name",
                )
            })?;
            let parent_dir = open_plain_directory(parent)?;
            nix::sys::stat::mkdirat(
                &parent_dir,
                name,
                nix::sys::stat::Mode::from_bits_truncate(0o700),
            )
            .map_err(std::io::Error::from)?;
            parent_dir.sync_all()?;
            let dir = open_private_context_dir(path)?;
            dir.set_permissions(fs::Permissions::from_mode(0o700))?;
            dir.sync_all()?;
            parent_dir.sync_all()?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn open_private_context_dir(path: &Path) -> std::io::Result<fs::File> {
    let dir = open_plain_directory_for_sync(path)?;
    if !dir.metadata()?.is_dir() {
        return Err(std::io::Error::other("path is not a directory"));
    }
    Ok(dir)
}

fn ensure_trailing_newline(content: &str) -> String {
    if content.is_empty() || content.ends_with('\n') {
        content.to_owned()
    } else {
        format!("{content}\n")
    }
}

fn require_socket_session_name(
    session_dir: &Path,
    session: &str,
) -> Result<(), SocketSessionRecordError> {
    if session_dir.file_name().and_then(|name| name.to_str()) == Some(session) {
        Ok(())
    } else {
        Err(SocketSessionRecordError::SessionMismatch)
    }
}

fn require_socket_session_files(session_dir: &Path) -> Result<(), SocketSessionRecordError> {
    for file in SESSION_REQUIRED_FILES {
        if !is_plain_existing_file(&session_dir.join(file)) {
            return Err(SocketSessionRecordError::MissingSessionFile(file));
        }
    }
    Ok(())
}

fn is_plain_existing_file(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_file())
}

fn is_plain_existing_dir(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir())
}

fn validate_socket_object_field(
    field: &'static str,
    value: &str,
) -> Result<(), SocketRequestError> {
    if is_object_name(value) {
        Ok(())
    } else {
        Err(SocketRequestError::InvalidField {
            field,
            value: value.to_owned(),
        })
    }
}
