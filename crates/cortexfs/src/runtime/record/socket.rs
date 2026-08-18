use super::*;

pub(crate) type SocketRecordResult<T> = Result<T, SocketSessionRecordError>;

/// Records durable filesystem effects for a parsed socket request.
///
/// `send` appends a user message to `messages.jsonl`, appends a canonical
/// `start` event to `events.jsonl`, marks the session active, and records a
/// supplied chroot `cwd` when present. `cancel` appends a cancelled `done`
/// event and marks the session cancelled. `resume`, `ping`, and temp sessions
/// do not mutate durable session files.
#[cfg(test)]
pub(crate) fn record_unindexed_socket_request_for_test(
    session_dir: &Path,
    request: &SocketRequest,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    record_socket_request(session_dir, request, None, None, None)
}

fn record_socket_request(
    session_dir: &Path,
    request: &SocketRequest,
    run_id: Option<&str>,
    preparation: Option<&OwnedSessionPreparation>,
    history: Option<&columnar::HistoryGuard<'_>>,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    match *request {
        SocketRequest::Send {
            ref id,
            ref session,
            scope,
            ref cwd,
            ref input,
            ..
        } => record_socket_send_to_session(
            session_dir,
            id,
            run_id.unwrap_or(id),
            session,
            scope,
            cwd.as_deref(),
            input,
            preparation,
            history,
        ),
        SocketRequest::Cancel { ref id } => record_socket_cancel_to_session(session_dir, id),
        SocketRequest::Tsh { .. }
        | SocketRequest::Resume { .. }
        | SocketRequest::Status { .. }
        | SocketRequest::Stop { .. }
        | SocketRequest::Ping => Err(SocketSessionRecordError::UnsupportedRequest),
    }
}

/// Records a durable socket `send` under `session_root/<session>/` and updates
/// the reserved session index files.
///
/// This is a filesystem helper for socket runtimes. It does not create
/// sessions, start models, or interpret provider state. The selected session
/// must already exist and have the stable durable files.
pub fn record_indexed_socket_send_to_session(
    session_root: &Path,
    request: &SocketRequest,
) -> Result<SocketSendOutcome<SocketSessionRecord>, IndexedSocketSessionRecordError> {
    record_indexed_socket_send(session_root, request, None)
}

pub(crate) fn record_prepared_indexed_socket_send_to_session(
    session_root: &Path,
    request: &SocketRequest,
    preparation: &OwnedSessionPreparation,
) -> Result<SocketSendOutcome<SocketSessionRecord>, IndexedSocketSessionRecordError> {
    record_indexed_socket_send(session_root, request, Some(preparation))
}

fn record_indexed_socket_send(
    session_root: &Path,
    request: &SocketRequest,
    preparation: Option<&OwnedSessionPreparation>,
) -> Result<SocketSendOutcome<SocketSessionRecord>, IndexedSocketSessionRecordError> {
    let (id, session, scope, cwd, input) = match *request {
        SocketRequest::Send {
            ref id,
            ref session,
            scope,
            ref cwd,
            ref input,
            ..
        } => (
            id.as_str(),
            session.as_str(),
            scope,
            cwd.as_deref(),
            input.as_str(),
        ),
        SocketRequest::Tsh { .. }
        | SocketRequest::Resume { .. }
        | SocketRequest::Status { .. }
        | SocketRequest::Cancel { .. }
        | SocketRequest::Stop { .. }
        | SocketRequest::Ping => {
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
    validate_socket_send(&session_dir, id, session, scope, input)
        .map_err(IndexedSocketSessionRecordError::Session)?;
    let history = columnar::HistoryGuard::exclusive(&session_dir).map_err(|_error| {
        IndexedSocketSessionRecordError::Session(SocketSessionRecordError::CannotRecord)
    })?;
    let by_cwd_key = cwd.and_then(session_index_key_for_cwd);
    let pending = match history
        .lookup_send(id, input, scope.as_str(), cwd)
        .map_err(|_error| {
            IndexedSocketSessionRecordError::Session(SocketSessionRecordError::CorruptHistory)
        })? {
        columnar::SendClaim::Vacant => None,
        columnar::SendClaim::Pending(send) => Some(send),
        columnar::SendClaim::Replay(frame) => {
            update_session_index(session_root, session, by_cwd_key.as_deref())
                .map_err(IndexedSocketSessionRecordError::Index)?;
            return Ok(SocketSendOutcome::Replayed(SocketSessionRecord::new(
                Vec::new(),
                vec![frame],
            )));
        }
        columnar::SendClaim::Conflict => {
            return Err(IndexedSocketSessionRecordError::Session(
                SocketSessionRecordError::RequestConflict,
            ));
        }
        columnar::SendClaim::Corrupt => {
            return Err(IndexedSocketSessionRecordError::Session(
                SocketSessionRecordError::CorruptHistory,
            ));
        }
    };

    preflight_session_index_update(session_root, session, by_cwd_key.as_deref(), None, None)
        .map_err(IndexedSocketSessionRecordError::Index)?;

    let run_id = if let Some(send) = pending.as_ref() {
        send.run_id().to_owned()
    } else {
        format!(
            "ctx-{}",
            support::receipt::random_hex::<16>().map_err(|_error| {
                IndexedSocketSessionRecordError::Session(SocketSessionRecordError::CannotRecord)
            })?
        )
    };
    let record = record_socket_request(
        &session_dir,
        request,
        Some(&run_id),
        preparation,
        Some(&history),
    )
    .map_err(IndexedSocketSessionRecordError::Session)?;
    update_session_index(session_root, session, by_cwd_key.as_deref())
        .map_err(IndexedSocketSessionRecordError::Index)?;

    Ok(SocketSendOutcome::Recorded(record))
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
    require_socket_session_files(session_dir)?;
    let history = columnar::HistoryGuard::exclusive(session_dir)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    history
        .refresh_claims()
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    let done = done_event_json(run_id, "ok");
    let record = record_assistant_response_locked(&history, session_dir, run_id, content, &done)?;
    history
        .refresh_claims()
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    set_session_state(session_dir, "done")?;
    Ok(record)
}

pub(crate) fn record_assistant_response_locked(
    history: &columnar::HistoryGuard<'_>,
    session_dir: &Path,
    run_id: &str,
    content: &str,
    done: &str,
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
        "run": run_id,
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
    history
        .append(columnar::Stream::Messages, &[&message])
        .and_then(|()| history.append(columnar::Stream::Events, &[&event, done]))
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    write_session_file(session_dir, "latest.md", &format!("{content}\n"))?;

    Ok(SocketSessionRecord::new(
        vec![message],
        vec![event, done.to_owned()],
    ))
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
    set_session_state_with_error(session_dir, "error", Some(denial.errno()))?;

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
        "run": run_id,
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
