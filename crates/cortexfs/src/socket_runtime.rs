const MAX_SOCKET_RUNTIME_SMALL_FILE_BYTES: u64 = 64 * 1024;
const MAX_SOCKET_RUNTIME_EVENTS_BYTES: u64 = 1024 * 1024;
const MAX_AGENT_EXECUTABLE_FRAME_BYTES: usize = 256 * 1024;
const SOCKET_REQUEST_READ_TIMEOUT: Duration = Duration::from_secs(5);

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

    if let Some(content) = local_agent_response(input) {
        let local_frames = local_agent_response_frames(id, content);
        if scope != SocketSessionScope::Temp {
            let session_dir = runtime.session_root.join(session);
            record_assistant_response_to_session(&session_dir, id, content)
                .map_err(SocketRuntimeError::Record)?;
        }
        for frame in &local_frames {
            write_socket_frame(stream, frame)?;
        }
        let mut frames = recorder_response.frames().to_vec();
        frames.extend(local_frames);
        return Ok(SocketRuntimeResponse::new(frames));
    }

    let agent_frames = run_agent_executable_streaming(stream, runtime, id, session, input)?;
    if scope != SocketSessionScope::Temp {
        let session_dir = runtime.session_root.join(session);
        record_tool_results_from_event_frames(&session_dir, id, &agent_frames)
            .map_err(SocketRuntimeError::Record)?;
        if let Some(text) = assistant_text_from_event_frames(&agent_frames) {
            record_assistant_response_to_session(&session_dir, id, &text)
                .map_err(SocketRuntimeError::Record)?;
        }
    }

    let mut frames = recorder_response.frames().to_vec();
    frames.extend(agent_frames);
    Ok(SocketRuntimeResponse::new(frames))
}

fn local_agent_response(input: &str) -> Option<&'static str> {
    if asks_for_tsh_usage(input) {
        return Some(TSH_USAGE_RESPONSE);
    }
    if asks_to_read_file_without_path(input) {
        return Some("请提供要读取的文件路径。");
    }
    None
}

const TSH_USAGE_RESPONSE: &str = "\
`tsh` 是 CortexFS 工具 shell，用来发现、加载和调用当前 agent 可见的工具。

常用命令：
- `tsh tools`：列出可见工具
- `tsh help`：查看 tsh 帮助
- `tsh help TOOL`：查看某个工具说明
- `tsh load TOOL`：把工具元数据加载进上下文
- `tsh loads`：查看已加载工具
- `tsh unload TOOL`：卸载未固定的工具元数据
- `tsh pin TOOL` / `tsh unpin TOOL`：固定或取消固定工具
- `tsh TOOL ARG...`：调用工具，例如 `tsh fs.read PATH`

当前 agent 只能原生调用 `tsh`；其他工具都要通过 `tsh` 间接调用。";

fn asks_for_tsh_usage(input: &str) -> bool {
    let normalized = input.to_ascii_lowercase();
    let mentions_tsh = normalized.contains("tsh");
    let asks_usage = input.contains("用法")
        || input.contains("怎么用")
        || input.contains("如何用")
        || input.contains("探索")
        || input.contains("查看")
        || input.contains("了解")
        || normalized.contains("usage")
        || normalized.contains("how to use")
        || normalized.contains("help");
    mentions_tsh && asks_usage
}

fn asks_to_read_file_without_path(input: &str) -> bool {
    let normalized = input.to_ascii_lowercase();
    let asks_read = input.contains("读文件")
        || input.contains("读取文件")
        || input.contains("看文件")
        || input.contains("打开文件")
        || normalized.contains("read file")
        || normalized.contains("read a file")
        || normalized.contains("open file")
        || normalized.contains("cat file");
    asks_read && !contains_file_path_hint(input)
}

fn contains_file_path_hint(input: &str) -> bool {
    input.split_whitespace().any(|word| {
        let word = word.trim_matches(|character: char| {
            matches!(
                character,
                '`' | '\'' | '"' | '“' | '”' | '‘' | '’' | ',' | '，' | ':' | '：' | ';' | '；'
            )
        });
        word.contains('/')
            || word.contains('\\')
            || word.starts_with('.')
            || word.starts_with('~')
            || word.rsplit_once('.').is_some_and(|(stem, ext)| {
                !stem.is_empty()
                    && ext.len() <= 12
                    && ext.bytes().all(|byte| byte.is_ascii_alphanumeric())
            })
    })
}

fn local_agent_response_frames(run_id: &str, content: &str) -> Vec<String> {
    let content_parts = vec![serde_json::json!({
        "type": "text",
        "text": content
    })];
    vec![
        serde_json::json!({
            "type": "message",
            "run": run_id,
            "role": "assistant",
            "content": content_parts
        })
        .to_string(),
        serde_json::json!({
            "type": "done",
            "run": run_id,
            "status": "ok"
        })
        .to_string(),
    ]
}

fn run_agent_executable_streaming(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    run_id: &str,
    session: &str,
    input: &str,
) -> Result<Vec<String>, SocketRuntimeError> {
    let history_messages = collect_history_messages_from_session(
        &runtime.session_root.join(session),
        MAX_HISTORY_MESSAGES_CHARS,
    );
    let agent_executable = open_agent_executable_no_follow(runtime.agent_executable)?;
    let mut command = Command::new(proc_fd_path(&agent_executable));
    command
        .arg(input)
        // The socket-activated service may hold provider credentials in its
        // own environment. Start executable agents from a clean environment
        // and then add only the derived agent view plus runtime-owned CTX_*
        // values so those service credentials cannot be inherited by agent
        // code or its descendants.
        .env_clear()
        .envs(
            runtime
                .env
                .iter()
                .map(|env| (env.0.as_str(), env.1.as_str())),
        )
        .env("CTX_AGENT", runtime.agent_name)
        .env("CTX_ROOT", runtime.ctx_root)
        .env("CTX_SOURCE", runtime.source_root)
        .env("CTX_RUN_ID", run_id)
        .env("CTX_SESSION", session)
        .env("CTX_AGENT_HISTORY_MESSAGES", history_messages)
        .stdout(Stdio::piped())
        .process_group(0);
    apply_agent_identity_to_command(&mut command, runtime.identity);
    let mut child = command
        .spawn()
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(SocketRuntimeError::CannotRunAgent)?;
    let (stdout_sender, stdout_receiver) = mpsc::sync_channel(MAX_AGENT_STDOUT_QUEUE_FRAMES);
    let reader = thread::spawn(move || {
        let mut stdout = BufReader::new(stdout);
        while let Some(line) = read_agent_executable_frame_line(&mut stdout)? {
            if stdout_sender.send(line).is_err() {
                break;
            }
        }
        Ok::<(), SocketRuntimeError>(())
    });
    let mut frames = Vec::new();
    let session_dir = runtime.session_root.join(session);
    let mut cancelled = false;
    loop {
        match stdout_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                if line.trim().is_empty() {
                    continue;
                }
                if !inspect_event_stream_jsonl(&line).is_ok() {
                    terminate_agent_process_group(&mut child);
                    let _ignored = child.wait();
                    return Err(SocketRuntimeError::InvalidAgentOutput);
                }
                if event_type(&line).as_deref() != Some("start") {
                    write_socket_frame(stream, &line)?;
                    frames.push(line);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                match reader.join() {
                    Ok(Ok(())) => break,
                    Ok(Err(error)) => {
                        terminate_agent_process_group(&mut child);
                        let _ignored = child.wait();
                        return Err(error);
                    }
                    Err(_error) => {
                        terminate_agent_process_group(&mut child);
                        let _ignored = child.wait();
                        return Err(SocketRuntimeError::CannotReadFrame);
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if agent_run_cancelled(&session_dir, run_id) {
                    cancelled = true;
                    terminate_agent_process_group(&mut child);
                    break;
                }
            }
        }
    }
    let status = child
        .wait()
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    if cancelled {
        return Ok(frames);
    }
    if !status.success() && frames.is_empty() {
        return Err(SocketRuntimeError::CannotRunAgent);
    }
    Ok(frames)
}

fn read_agent_executable_frame_line(
    reader: &mut impl BufRead,
) -> Result<Option<String>, SocketRuntimeError> {
    let mut bytes = Vec::new();
    let limit = u64::try_from(MAX_AGENT_EXECUTABLE_FRAME_BYTES.saturating_add(1))
        .map_err(|_error| SocketRuntimeError::CannotReadFrame)?;
    let read = reader
        .take(limit)
        .read_until(b'\n', &mut bytes)
        .map_err(|_error| SocketRuntimeError::CannotReadFrame)?;
    if read == 0 {
        return Ok(None);
    }
    if bytes.len() > MAX_AGENT_EXECUTABLE_FRAME_BYTES {
        return Err(SocketRuntimeError::InvalidAgentOutput);
    }
    if bytes.ends_with(b"\n") {
        bytes.pop();
        if bytes.ends_with(b"\r") {
            bytes.pop();
        }
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_error| SocketRuntimeError::InvalidAgentOutput)
}

fn apply_agent_identity_to_command(command: &mut Command, identity: &AgentUnixIdentity) {
    // Non-root test/development invocations retain their existing uid/gid. The
    // packaged socket service has already dropped supplementary groups to the
    // derived agent identity; setting child uid/gid here keeps the helper safe
    // for privileged callers without reintroducing unsafe pre-exec code.
    if nix::unistd::geteuid().is_root() {
        command.gid(identity.gid()).uid(identity.uid());
    }
}

fn open_agent_executable_no_follow(path: &Path) -> Result<fs::File, SocketRuntimeError> {
    if !path.is_absolute() {
        return Err(SocketRuntimeError::InvalidAgentExecutable);
    }
    let parent = path
        .parent()
        .ok_or(SocketRuntimeError::InvalidAgentExecutable)?;
    let parent_dir = open_socket_runtime_plain_directory(parent)
        .map_err(|_error| SocketRuntimeError::InvalidAgentExecutable)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(SocketRuntimeError::InvalidAgentExecutable)?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|_error| SocketRuntimeError::InvalidAgentExecutable)?;
    let file = fs::File::from(file_fd);
    let metadata = file
        .metadata()
        .map_err(|_error| SocketRuntimeError::InvalidAgentExecutable)?;
    if metadata.is_file() {
        Ok(file)
    } else {
        Err(SocketRuntimeError::InvalidAgentExecutable)
    }
}

fn proc_fd_path(file: &fs::File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}

fn terminate_agent_process_group(child: &mut Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        signal_agent_process_group(pid, nix::sys::signal::Signal::SIGTERM);
        for _attempt in 0..5 {
            if child.try_wait().ok().flatten().is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        signal_agent_process_group(pid, nix::sys::signal::Signal::SIGKILL);
    }
    let _ignored = child.kill();
}

fn signal_agent_process_group(pid: i32, signal: nix::sys::signal::Signal) {
    let _ignored = nix::sys::signal::kill(nix::unistd::Pid::from_raw(-pid), signal);
}

fn event_type(line: &str) -> Option<String> {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_owned))
}

fn agent_run_cancelled(session_dir: &Path, run_id: &str) -> bool {
    let Ok(state) = socket_runtime_read_plain_text_file(
        &session_dir.join("state"),
        MAX_SOCKET_RUNTIME_SMALL_FILE_BYTES,
    ) else {
        return false;
    };
    if state.trim() != "cancelled" {
        return false;
    }
    let Ok(events) = socket_runtime_read_plain_text_file(
        &session_dir.join("events.jsonl"),
        MAX_SOCKET_RUNTIME_EVENTS_BYTES,
    ) else {
        return false;
    };
    events.lines().any(|line| {
        serde_json::from_str::<Value>(line).is_ok_and(|value| {
            value.get("type").and_then(Value::as_str) == Some("done")
                && value.get("run").and_then(Value::as_str) == Some(run_id)
                && value.get("status").and_then(Value::as_str) == Some("cancelled")
        })
    })
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

fn record_tool_results_from_event_frames(
    session_dir: &Path,
    run_id: &str,
    frames: &[String],
) -> Result<(), SocketSessionRecordError> {
    let mut calls = Vec::new();
    for frame in frames {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("tool_call") {
            continue;
        }
        let Some(id) = value.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(name) = value.get("name").and_then(Value::as_str) else {
            continue;
        };
        calls.push((id.to_owned(), name.to_owned()));
    }

    for frame in frames {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            continue;
        };
        if value.get("type").and_then(Value::as_str) != Some("message")
            || value.get("role").and_then(Value::as_str) != Some("tool")
        {
            continue;
        }
        let event_tool_name = value.get("name").and_then(Value::as_str);
        let Some(parts) = value.get("content").and_then(Value::as_array) else {
            continue;
        };
        for part in parts {
            if part.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let Some(tool_call_id) = part.get("tool_call_id").and_then(Value::as_str) else {
                continue;
            };
            let Some(tool_name) = event_tool_name
                .or_else(|| tool_name_for_call(&calls, tool_call_id).map(String::as_str))
            else {
                continue;
            };
            let content = tool_result_content_text(part.get("content"));
            record_tool_execution_result_to_session(
                session_dir,
                run_id,
                tool_call_id,
                tool_name,
                &content,
            )?;
        }
    }
    Ok(())
}

fn tool_name_for_call<'a>(calls: &'a [(String, String)], tool_call_id: &str) -> Option<&'a String> {
    calls
        .iter()
        .find_map(|call| (call.0 == tool_call_id).then_some(&call.1))
}

fn tool_result_content_text(content: Option<&Value>) -> String {
    if let Some(value) = content.and_then(Value::as_str) {
        return value.to_owned();
    }
    content.map_or_else(String::new, Value::to_string)
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
    let restore_blocking = stream
        .read_timeout()
        .map_err(|_error| SocketRuntimeError::CannotReadFrame)?
        .is_none();
    if restore_blocking {
        stream
            .set_read_timeout(Some(SOCKET_REQUEST_READ_TIMEOUT))
            .map_err(|_error| SocketRuntimeError::CannotReadFrame)?;
    }
    let frame = read_socket_request_frame_body(stream);
    if restore_blocking {
        stream
            .set_read_timeout(None)
            .map_err(|_error| SocketRuntimeError::CannotReadFrame)?;
    }
    frame
}

fn read_socket_request_frame_body(stream: &mut UnixStream) -> Result<String, SocketRuntimeError> {
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
    let events = socket_runtime_read_plain_text_file(
        &session_root.join(session).join("events.jsonl"),
        MAX_SOCKET_RUNTIME_EVENTS_BYTES,
    )
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
    match socket_runtime_read_plain_text_file(&current_path, MAX_SOCKET_RUNTIME_SMALL_FILE_BYTES) {
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

fn socket_runtime_read_plain_text_file(path: &Path, limit: u64) -> std::io::Result<String> {
    let mut file = open_socket_runtime_plain_file(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "socket runtime file is too large or not a plain file",
        ));
    }
    let len = usize::try_from(metadata.len())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.utf8_error()))
}

fn open_socket_runtime_plain_file(path: &Path) -> std::io::Result<fs::File> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_socket_runtime_plain_directory(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid file name"))?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    Ok(fs::File::from(file_fd))
}

fn open_socket_runtime_plain_directory(path: &Path) -> std::io::Result<fs::File> {
    let mut directory = if path.is_absolute() {
        open_socket_runtime_single_plain_directory(Path::new("/"))?
    } else {
        open_socket_runtime_single_plain_directory(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid directory name")
                })?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )
                .map_err(std::io::Error::from)?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_socket_runtime_single_plain_directory(path: &Path) -> std::io::Result<fs::File> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ));
    }
    Ok(directory)
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
