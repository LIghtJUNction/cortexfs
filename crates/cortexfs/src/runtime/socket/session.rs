use super::*;

pub(crate) fn handle_socket_send(
    session_root: &Path,
    default_cwd: &str,
    model: Option<&str>,
    request: &SocketRequest,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let &SocketRequest::Send {
        ref id,
        ref session,
        scope,
        ref cwd,
        ref input,
        ..
    } = request
    else {
        return Err(SocketRuntimeError::Record(
            SocketSessionRecordError::UnsupportedRequest,
        ));
    };
    let effective_cwd = cwd.as_deref().unwrap_or(default_cwd);
    if scope == SocketSessionScope::Temp {
        if !is_stable_chroot_absolute_path(effective_cwd) {
            return Err(SocketRuntimeError::SessionLayout(
                DurableSessionLayoutError::InvalidCwd,
            ));
        }
        return Ok(SocketRuntimeResponse::new(vec![socket_start_frame(
            id, model,
        )]));
    }

    ensure_durable_session_layout(session_root, session, effective_cwd, model, scope)
        .map_err(SocketRuntimeError::SessionLayout)?;
    let durable_request = SocketRequest::Send {
        id: id.to_owned(),
        session: session.to_owned(),
        scope,
        cwd: Some(effective_cwd.to_owned()),
        workspace: None,
        input: input.to_owned(),
    };
    let record = record_indexed_socket_send_to_session(session_root, &durable_request)
        .map_err(SocketRuntimeError::IndexedRecord)?;
    Ok(SocketRuntimeResponse::new(record.events().to_vec()))
}

pub(crate) fn handle_socket_resume(
    session_root: &Path,
    session: &str,
    after: Option<&str>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    if !is_object_name(session) {
        return Err(SocketRuntimeError::InvalidSessionName);
    }
    let events = support::plain::read_small_text_file(
        &session_root.join(session).join("events.jsonl"),
        MAX_SOCKET_RUNTIME_EVENTS_BYTES,
    )
    .map_err(|_error| SocketRuntimeError::CannotReadEvents)?;
    Ok(SocketRuntimeResponse::new(resume_event_frames(
        &events, after,
    )))
}

pub(crate) fn handle_socket_cancel(
    session_root: &Path,
    run_id: &str,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let session = current_or_default_session_name(session_root)?;
    let session_dir = session_root.join(session);
    let request = SocketRequest::Cancel {
        id: run_id.to_owned(),
    };
    let record = record_socket_request_to_session(&session_dir, &request)
        .map_err(SocketRuntimeError::Record)?;
    Ok(SocketRuntimeResponse::new(record.events().to_vec()))
}

pub(crate) fn resume_event_frames(events: &str, after: Option<&str>) -> Vec<String> {
    let mut include = after.is_none();
    let mut frames = Vec::new();
    for line in events.lines().filter(|line| !line.trim().is_empty()) {
        if include {
            frames.push(line.to_owned());
            continue;
        }
        if after.is_some_and(|cursor| event_id_matches(line, cursor)) {
            include = true;
        }
    }
    frames
}

pub(crate) fn event_id_matches(line: &str, cursor: &str) -> bool {
    serde_json::from_str::<Value>(line).is_ok_and(|value| {
        value
            .as_object()
            .and_then(|object| object.get("id"))
            .and_then(Value::as_str)
            == Some(cursor)
    })
}

pub(crate) fn current_or_default_session_name(
    session_root: &Path,
) -> Result<String, SocketRuntimeError> {
    let current_path = session_root.join("index").join("current");
    match support::plain::read_small_text_file(&current_path, MAX_SOCKET_RUNTIME_SMALL_FILE_BYTES) {
        Ok(value) => {
            let session = value.trim();
            if is_object_name(session) {
                Ok(session.to_owned())
            } else {
                Err(SocketRuntimeError::InvalidSessionName)
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok("default".to_owned()),
        Err(_error) => Err(SocketRuntimeError::CannotReadEvents),
    }
}

pub(crate) fn socket_start_frame(run_id: &str, model: Option<&str>) -> String {
    let mut value = serde_json::json!({
        "type": "start",
        "id": run_id,
        "run": run_id
    });
    if let Some(model) = model
        && let Some(object) = value.as_object_mut()
    {
        object.insert("model".to_owned(), serde_json::json!(model));
    }
    value.to_string()
}

pub(crate) fn socket_pong_frame() -> String {
    serde_json::json!({"type": "pong"}).to_string()
}
