fn handle_agent_executable_socket_request_frame_streaming(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    frame: &str,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let debug = socket_debug_timing_from_frame(frame);
    let request = parse_socket_request_frame(frame).map_err(SocketRuntimeError::Request)?;
    let SocketRequest::Send {
        ref id,
        ref session,
        scope,
        ref cwd,
        ref workspace,
        ref input,
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
    if let Some(debug) = debug {
        write_socket_debug_timing_frame(stream, debug, "socket_send_received")?;
    }
    let history_messages = collect_history_messages_from_session(
        &runtime.session_root.join(session),
        MAX_HISTORY_MESSAGES_CHARS,
    );
    let tool_context = agent_tool_context_for_request(cwd.as_deref(), workspace.as_deref());
    if let Some(debug) = debug {
        write_socket_debug_timing_frame(stream, debug, "history_collected")?;
    }

    let recorder_response = handle_socket_request(
        runtime.session_root,
        runtime.default_cwd,
        runtime.model,
        &request,
    )?;
    write_socket_runtime_response(stream, &recorder_response)?;
    if let Some(debug) = debug {
        write_socket_debug_timing_frame(stream, debug, "session_recorded")?;
    }

    let agent_frames = run_agent_executable_streaming(
        stream,
        runtime,
        AgentExecutableRunRequest {
            run_id: id,
            session,
            cwd: cwd.as_deref(),
            workspace: workspace.as_deref(),
            input,
            history_messages: &history_messages,
            tool_context: &tool_context,
            debug,
        },
    )?;
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

fn run_agent_executable_streaming(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    request: AgentExecutableRunRequest<'_>,
) -> Result<Vec<String>, SocketRuntimeError> {
    let agent_executable = open_agent_executable_no_follow(runtime.agent_executable)?;
    let mut command = agent_executable_socket_command(runtime, &agent_executable, request);
    apply_socket_debug_timing_env(&mut command, request.debug);
    apply_agent_identity_to_command(&mut command, runtime.identity);
    command.stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    write_optional_socket_debug_timing_frame(stream, request.debug, "agent_spawned")?;
    let stdout = child
        .stdout
        .take()
        .ok_or(SocketRuntimeError::CannotRunAgent)?;
    let stderr = child
        .stderr
        .take()
        .ok_or(SocketRuntimeError::CannotRunAgent)?;
    let stderr_reader = thread::spawn(move || read_agent_executable_stderr_limited(stderr));
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
    let session_dir = runtime.session_root.join(request.session);
    let mut cancelled = false;
    let mut saw_agent_frame = false;
    loop {
        match stdout_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                if line.trim().is_empty() {
                    continue;
                }
                if is_socket_debug_timing_frame(&line, request.debug) {
                    write_socket_frame(stream, &line)?;
                    continue;
                }
                if !saw_agent_frame {
                    write_optional_socket_debug_timing_frame(
                        stream,
                        request.debug,
                        "first_agent_frame",
                    )?;
                    saw_agent_frame = true;
                }
                if !inspect_event_stream_jsonl(&line).is_ok() {
                    if frames.is_empty() {
                        terminate_agent_process_group(&mut child);
                        let _ignored = child.wait();
                        return Err(SocketRuntimeError::InvalidAgentOutput);
                    }
                    let wrapped = agent_plain_text_frame(request.run_id, &line);
                    write_socket_frame(stream, &wrapped)?;
                    frames.push(wrapped);
                    continue;
                }
                if event_type(&line).as_deref() != Some("start") {
                    write_socket_frame(stream, &line)?;
                    frames.push(line);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => match reader.join() {
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
            },
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if agent_run_cancelled(&session_dir, request.run_id) {
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
        let stderr = match stderr_reader.join() {
            Ok(Ok(stderr)) => stderr,
            Ok(Err(_error)) => String::new(),
            Err(_error) => String::new(),
        };
        let frames = agent_process_failed_frames(request.run_id, &stderr);
        for frame in &frames {
            write_socket_frame(stream, frame)?;
        }
        return Ok(frames);
    }
    Ok(frames)
}

fn read_agent_executable_stderr_limited(stderr: impl Read) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    stderr
        .take(MAX_AGENT_EXECUTABLE_STDERR_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > usize::try_from(MAX_AGENT_EXECUTABLE_STDERR_BYTES).unwrap_or(usize::MAX) {
        bytes.truncate(usize::try_from(MAX_AGENT_EXECUTABLE_STDERR_BYTES).unwrap_or(usize::MAX));
    }
    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}

fn agent_process_failed_frames(run_id: &str, stderr: &str) -> Vec<String> {
    let message = if stderr.is_empty() {
        "agent process failed before emitting events"
    } else {
        stderr
    };
    vec![
        serde_json::json!({
            "type": "error",
            "run": run_id,
            "code": "EIO",
            "message": message
        })
        .to_string(),
        serde_json::json!({
            "type": "done",
            "run": run_id,
            "status": "error"
        })
        .to_string(),
    ]
}

fn agent_plain_text_frame(run_id: &str, text: &str) -> String {
    serde_json::json!({
        "type": "delta",
        "run": run_id,
        "text": text
    })
    .to_string()
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

#[derive(Clone, Copy)]
struct AgentExecutableRunRequest<'a> {
    run_id: &'a str,
    session: &'a str,
    cwd: Option<&'a str>,
    workspace: Option<&'a str>,
    input: &'a str,
    history_messages: &'a str,
    tool_context: &'a str,
    debug: Option<SocketDebugTiming>,
}

fn agent_tool_context_for_request(cwd: Option<&str>, workspace: Option<&str>) -> String {
    let mut context = default_agent_tool_context();
    context.push_str("\n\nCurrent request context:\n");
    context.push_str("- Sandbox cwd: ");
    context.push_str(&prompt_quoted(cwd.unwrap_or("/workspace")));
    context.push('\n');
    match workspace.filter(|value| host_path::is_absolute_host_workspace_path(value)) {
        Some(workspace) => {
            context.push_str("- Host workspace mounted at `/workspace`: ");
            context.push_str(&prompt_quoted(workspace));
            context.push('\n');
        }
        None => {
            context.push_str("- Host workspace mounted at `/workspace`: unknown\n");
        }
    }
    context
}

fn prompt_quoted(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_error| "\"\"".to_owned())
}
