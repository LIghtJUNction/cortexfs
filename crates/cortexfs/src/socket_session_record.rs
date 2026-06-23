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

    let session_dir = session_root.join(session);
    let record = record_socket_request_to_session(&session_dir, request)
        .map_err(IndexedSocketSessionRecordError::Session)?;
    let by_cwd_key = cwd.and_then(session_index_key_for_cwd);
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
    fs::create_dir_all(child_dir.join("artifact"))
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
        if !parent_session_dir.join(file).is_file() {
            return Err(ChildContextRecordError::MissingParentSession);
        }
    }
    if !parent_session_dir.join("context").is_dir() {
        return Err(ChildContextRecordError::MissingParentSession);
    }
    Ok(())
}

fn require_child_context_files(child_dir: &Path) -> Result<(), ChildContextRecordError> {
    for file in CHILD_RESULT_REQUIRED_FILES {
        if !child_dir.join(file).is_file() {
            return Err(ChildContextRecordError::MissingParentSession);
        }
    }
    for dir in CHILD_RESULT_REQUIRED_DIRS {
        if !child_dir.join(dir).is_dir() {
            return Err(ChildContextRecordError::MissingParentSession);
        }
    }
    Ok(())
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
    if path.exists() {
        return if path.is_file() {
            Ok(())
        } else {
            Err(std::io::Error::other("path is not a regular file"))
        };
    }
    fs::write(path, content)
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
        if !session_dir.join(file).is_file() {
            return Err(SocketSessionRecordError::MissingSessionFile(file));
        }
    }
    Ok(())
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
