/// Handles one JSONL socket request frame against a durable session root.
///
/// This is the reusable core for a future Unix socket loop: it parses one
/// request, applies `CortexFS` session-file semantics, and returns canonical
/// response frames. It does not call a model provider and does not execute
/// tools.
pub fn handle_socket_request_frame(
    session_root: &Path,
    default_cwd: &str,
    model: Option<&str>,
    frame: &str,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let request = parse_socket_request_frame(frame).map_err(SocketRuntimeError::Request)?;
    handle_socket_request(session_root, default_cwd, model, &request)
}

/// Handles one parsed socket request against a durable session root.
pub fn handle_socket_request(
    session_root: &Path,
    default_cwd: &str,
    model: Option<&str>,
    request: &SocketRequest,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    match *request {
        SocketRequest::Ping => Ok(SocketRuntimeResponse::new(vec![socket_pong_frame()])),
        SocketRequest::Send { .. } => handle_socket_send(session_root, default_cwd, model, request),
        SocketRequest::Resume {
            ref session,
            ref after,
        } => handle_socket_resume(session_root, session, after.as_deref()),
        SocketRequest::Cancel { ref id } => handle_socket_cancel(session_root, id),
    }
}

/// Builds a canonical socket error response frame from a runtime error.
#[must_use]
pub fn socket_runtime_error_response(error: &SocketRuntimeError) -> SocketRuntimeResponse {
    SocketRuntimeResponse::new(vec![
        serde_json::json!({
            "type": "error",
            "code": error.errno(),
            "message": error.errno()
        })
        .to_string(),
    ])
}

/// Accepts and serves one Unix socket connection.
///
/// This is a bounded runtime helper for `name.sock` implementations. It does
/// not loop, spawn, supervise, or watch files; callers decide process lifetime.
pub fn serve_unix_socket_listener_once(
    listener: &UnixListener,
    peer_policy: Option<SocketPeerPolicy>,
    session_root: &Path,
    default_cwd: &str,
    model: Option<&str>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let (mut stream, _addr) = listener
        .accept()
        .map_err(|_error| SocketRuntimeError::CannotAcceptConnection)?;
    serve_unix_socket_stream_once(&mut stream, peer_policy, session_root, default_cwd, model)
}

/// Accepts one Unix socket connection and dispatches `send` to an agent executable.
///
/// This is the reference socket-activated agent runtime path. It preserves the
/// durable socket request semantics, then runs the ABI executable object for
/// `send` requests and returns its canonical JSONL events to the client.
pub fn serve_agent_executable_socket_listener_once(
    listener: &UnixListener,
    peer_policy: Option<SocketPeerPolicy>,
    runtime: AgentExecutableSocketRuntime<'_>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let (mut stream, _addr) = listener
        .accept()
        .map_err(|_error| SocketRuntimeError::CannotAcceptConnection)?;
    serve_agent_executable_socket_stream_once(&mut stream, peer_policy, runtime)
}

/// Serves one connected stream and dispatches `send` to an agent executable.
pub fn serve_agent_executable_socket_stream_once(
    stream: &mut UnixStream,
    peer_policy: Option<SocketPeerPolicy>,
    runtime: AgentExecutableSocketRuntime<'_>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    serve_socket_stream_with(stream, peer_policy, |stream, frame| {
        handle_agent_executable_socket_request_frame_streaming(stream, runtime, frame)
    })
}

/// Serves one connected Unix socket stream request.
///
/// This helper enforces optional kernel peer credentials before reading a
/// single JSONL frame, then writes either the request response or a stable
/// error frame. It is intentionally one-shot; process supervision and accept
/// loops remain outside the ABI.
pub fn serve_unix_socket_stream_once(
    stream: &mut UnixStream,
    peer_policy: Option<SocketPeerPolicy>,
    session_root: &Path,
    default_cwd: &str,
    model: Option<&str>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    serve_socket_stream_with(stream, peer_policy, |stream, frame| {
        let response = handle_socket_request_frame(session_root, default_cwd, model, frame)?;
        write_socket_runtime_response(stream, &response)?;
        Ok(response)
    })
}

fn serve_socket_stream_with(
    stream: &mut UnixStream,
    peer_policy: Option<SocketPeerPolicy>,
    dispatch: impl FnOnce(&mut UnixStream, &str) -> Result<SocketRuntimeResponse, SocketRuntimeError>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    if let Some(policy) = peer_policy {
        let peer = peer_credentials(stream).map_err(SocketRuntimeError::PeerCredential)?;
        if !policy.allows(peer) {
            let error = SocketRuntimeError::PeerDenied;
            let response = socket_runtime_error_response(&error);
            write_socket_runtime_response(stream, &response)?;
            return Err(error);
        }
    }

    let frame = match read_socket_request_frame_from_stream(stream) {
        Ok(frame) => frame,
        Err(error) => {
            let response = socket_runtime_error_response(&error);
            write_socket_runtime_response(stream, &response)?;
            return Err(error);
        }
    };
    match dispatch(stream, &frame) {
        Ok(response) => Ok(response),
        Err(error) => {
            let response = socket_runtime_error_response(&error);
            write_socket_runtime_response(stream, &response)?;
            Err(error)
        }
    }
}

fn handle_agent_executable_socket_request_frame_streaming(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    frame: &str,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let request = parse_socket_request_frame(frame).map_err(SocketRuntimeError::Request)?;
    let SocketRequest::Send {
        ref id,
        ref session,
        scope,
        ref input,
        ..
    } = request
    else {
        let response = handle_socket_request(
            runtime.session_root,
            runtime.default_cwd,
            runtime.model,
            &request,
        )?;
        write_socket_runtime_response(stream, &response)?;
        return Ok(response);
    };

    let recorder_response = handle_socket_request(
        runtime.session_root,
        runtime.default_cwd,
        runtime.model,
        &request,
    )?;
    write_socket_runtime_response(stream, &recorder_response)?;

    let agent_frames = run_agent_executable_streaming(stream, runtime, id, session, input)?;
    if scope != SocketSessionScope::Temp
        && let Some(text) = assistant_text_from_event_frames(&agent_frames)
    {
        let session_dir = runtime.session_root.join(session);
        record_assistant_response_to_session(&session_dir, id, &text)
            .map_err(SocketRuntimeError::Record)?;
    }

    let mut frames = recorder_response.frames().to_vec();
    frames.extend(agent_frames);
    Ok(SocketRuntimeResponse::new(frames))
}

fn run_agent_executable_streaming(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    run_id: &str,
    session: &str,
    input: &str,
) -> Result<Vec<String>, SocketRuntimeError> {
    let mut child = Command::new(runtime.agent_executable)
        .arg(input)
        .env("CTX_AGENT", runtime.agent_name)
        .env("CTX_ROOT", runtime.ctx_root)
        .env("CTX_SOURCE", runtime.source_root)
        .env("CTX_RUN_ID", run_id)
        .env("CTX_SESSION", session)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(SocketRuntimeError::CannotRunAgent)?;
    let mut frames = Vec::new();
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|_error| SocketRuntimeError::CannotReadFrame)?;
        if line.trim().is_empty() {
            continue;
        }
        if !inspect_event_stream_jsonl(&line).is_ok() {
            return Err(SocketRuntimeError::InvalidAgentOutput);
        }
        if event_type(&line).as_deref() != Some("start") {
            write_socket_frame(stream, &line)?;
            frames.push(line);
        }
    }
    let status = child
        .wait()
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    if !status.success() && frames.is_empty() {
        return Err(SocketRuntimeError::CannotRunAgent);
    }
    Ok(frames)
}

fn event_type(line: &str) -> Option<String> {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_owned))
}

fn assistant_text_from_event_frames(frames: &[String]) -> Option<String> {
    let mut output = String::new();
    for frame in frames {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            continue;
        };
        let event_type = value.get("type").and_then(Value::as_str);
        if matches!(event_type, Some("delta" | "reasoning_delta"))
            && let Some(text) = value.get("text").and_then(Value::as_str)
        {
            output.push_str(text);
            continue;
        }
        if matches!(event_type, Some("message" | "reasoning_message"))
            && value.get("role").and_then(Value::as_str) == Some("assistant")
            && let Some(text) = message_event_text(&value)
        {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&text);
        }
    }
    (!output.is_empty()).then_some(output)
}

fn message_event_text(value: &Value) -> Option<String> {
    let parts = value.get("content")?.as_array()?;
    let mut text = String::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str) == Some("text")
            && let Some(value) = part.get("text").and_then(Value::as_str)
        {
            text.push_str(value);
        }
    }
    (!text.is_empty()).then_some(text)
}

fn read_socket_request_frame_from_stream(
    stream: &mut UnixStream,
) -> Result<String, SocketRuntimeError> {
    let mut buffer = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                buffer.push(byte[0]);
                if buffer.len() > MAX_SOCKET_FRAME_BYTES {
                    return Err(SocketRuntimeError::Request(
                        SocketRequestError::FrameTooLarge {
                            bytes: buffer.len(),
                        },
                    ));
                }
                if byte[0] == b'\n' {
                    break;
                }
            }
            Err(_error) => return Err(SocketRuntimeError::CannotReadFrame),
        }
    }
    String::from_utf8(buffer)
        .map_err(|_error| SocketRuntimeError::Request(SocketRequestError::InvalidJson))
}

fn write_socket_runtime_response(
    stream: &mut UnixStream,
    response: &SocketRuntimeResponse,
) -> Result<(), SocketRuntimeError> {
    stream
        .write_all(response.jsonl().as_bytes())
        .and_then(|()| stream.flush())
        .map_err(|_error| SocketRuntimeError::CannotWriteResponse)
}

fn write_socket_frame(stream: &mut UnixStream, frame: &str) -> Result<(), SocketRuntimeError> {
    stream
        .write_all(frame.as_bytes())
        .and_then(|()| stream.write_all(b"\n"))
        .and_then(|()| stream.flush())
        .map_err(|_error| SocketRuntimeError::CannotWriteResponse)
}

fn handle_socket_send(
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
        input: input.to_owned(),
    };
    let record = record_indexed_socket_send_to_session(session_root, &durable_request)
        .map_err(SocketRuntimeError::IndexedRecord)?;
    Ok(SocketRuntimeResponse::new(record.events().to_vec()))
}

fn handle_socket_resume(
    session_root: &Path,
    session: &str,
    after: Option<&str>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    if !is_object_name(session) {
        return Err(SocketRuntimeError::InvalidSessionName);
    }
    let events = fs::read_to_string(session_root.join(session).join("events.jsonl"))
        .map_err(|_error| SocketRuntimeError::CannotReadEvents)?;
    Ok(SocketRuntimeResponse::new(resume_event_frames(
        &events, after,
    )))
}

fn handle_socket_cancel(
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

fn resume_event_frames(events: &str, after: Option<&str>) -> Vec<String> {
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

fn event_id_matches(line: &str, cursor: &str) -> bool {
    serde_json::from_str::<Value>(line).is_ok_and(|value| {
        value
            .as_object()
            .and_then(|object| object.get("id"))
            .and_then(Value::as_str)
            == Some(cursor)
    })
}

fn current_or_default_session_name(session_root: &Path) -> Result<String, SocketRuntimeError> {
    let current_path = session_root.join("index").join("current");
    match fs::read_to_string(current_path) {
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

fn socket_start_frame(run_id: &str, model: Option<&str>) -> String {
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

fn socket_pong_frame() -> String {
    serde_json::json!({"type": "pong"}).to_string()
}
