use super::*;
use std::os::unix::fs::MetadataExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

struct RunControlServer {
    shutdown: Arc<AtomicBool>,
    startup: mpsc::Receiver<Result<(), runtime::control::RunCapabilityError>>,
    join: Option<thread::JoinHandle<Result<(), runtime::control::RunCapabilityError>>>,
}

impl RunControlServer {
    fn finish(&mut self) -> Result<(), runtime::control::RunCapabilityError> {
        self.shutdown.store(true, Ordering::Release);
        self.join
            .take()
            .ok_or(runtime::control::RunCapabilityError::CannotAccept)?
            .join()
            .map_err(|_error| runtime::control::RunCapabilityError::CannotAccept)?
    }
}

impl Drop for RunControlServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(join) = self.join.take() {
            let _ignored = join.join();
        }
    }
}

pub(crate) fn handle_agent_executable_socket_request_frame_streaming(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    stop: Option<&dyn AgentStopHandler>,
    peer_uid: u32,
    frame: &str,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let debug = socket_debug_timing_from_frame(frame);
    let request = parse_socket_request_frame(frame).map_err(SocketRuntimeError::Request)?;
    if let SocketRequest::Stop { ref agent } = request {
        if agent != runtime.agent_name {
            return Err(SocketRuntimeError::PeerDenied);
        }
        let handler = stop.ok_or(SocketRuntimeError::Record(
            SocketSessionRecordError::UnsupportedRequest,
        ))?;
        let prepared = handler.preflight(agent, peer_uid)?;
        let response = SocketRuntimeResponse::new(vec![
            serde_json::json!({ "type": "accepted", "op": "stop", "agent": agent }).to_string(),
        ]);
        write_socket_runtime_response(stream, &response)?;
        prepared
            .execute()
            .map_err(|_error| SocketRuntimeError::PostAcceptStop)?;
        return Ok(response);
    }
    let SocketRequest::Send {
        ref id,
        ref session,
        scope,
        ref cwd,
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
    let session_dir = runtime.session_root.join(session);
    let preparation = if scope == SocketSessionScope::Private {
        Some(
            prepare_owned_durable_session(
                runtime.session_root,
                session,
                cwd.as_deref().unwrap_or(runtime.default_cwd),
                runtime.model,
                scope,
                runtime.identity.uid(),
                runtime.identity.gid(),
            )
            .map_err(|_error| SocketRuntimeError::CannotRunAgent)?,
        )
    } else {
        None
    };
    let history_messages =
        collect_history_messages_from_session(&session_dir, MAX_HISTORY_MESSAGES_CHARS);
    let tool_context = agent_tool_context_for_request(cwd.as_deref());
    let recorder_response = handle_socket_send(
        runtime.session_root,
        runtime.default_cwd,
        runtime.model,
        &request,
        preparation.as_ref(),
    )?;
    if let Some(debug) = debug {
        write_socket_debug_timing_frame(stream, debug, "socket_send_received")?;
        write_socket_debug_timing_frame(stream, debug, "history_collected")?;
    }
    write_socket_runtime_response(stream, &recorder_response)?;
    if let Some(debug) = debug {
        let mut client_connected = true;
        write_while_connected(&mut client_connected, || {
            write_socket_debug_timing_frame(stream, debug, "session_recorded")
        })?;
    }

    let run_request = AgentExecutableRunRequest {
        run_id: id,
        session,
        cwd: cwd.as_deref(),
        input,
        history_messages: &history_messages,
        tool_context: &tool_context,
        debug,
        envelope: None,
        step: 0,
    };
    let agent_outcome = if agent_uses_sdk_envelope(runtime)? {
        run_agent_envelope_loop(stream, runtime, run_request)?
    } else {
        run_agent_executable_streaming(stream, runtime, run_request)?
    };
    record_owned_child_completion(runtime, session, id, &agent_outcome)?;
    let agent_frames = agent_outcome.frames;
    if scope != SocketSessionScope::Temp {
        record_approval_frames(&session_dir, id, &agent_frames)
            .map_err(SocketRuntimeError::Record)?;
        record_tool_results_from_event_frames(&session_dir, id, &agent_frames)
            .map_err(SocketRuntimeError::Record)?;
        let terminal_error = record_agent_error_from_event_frames(&session_dir, id, &agent_frames)
            .map_err(SocketRuntimeError::Record)?;
        if !terminal_error && let Some(text) = assistant_text_from_event_frames(&agent_frames) {
            record_assistant_response_to_session(&session_dir, id, &text)
                .map_err(SocketRuntimeError::Record)?;
        }
    }

    let mut frames = recorder_response.frames().to_vec();
    frames.extend(agent_frames);
    Ok(SocketRuntimeResponse::new(frames))
}

fn agent_uses_sdk_envelope(
    runtime: AgentExecutableSocketRuntime<'_>,
) -> Result<bool, SocketRuntimeError> {
    let path = runtime
        .source_root
        .join("agent")
        .join(format!("{}.d", runtime.agent_name))
        .join("abi");
    match support::plain::read_small_text_file(&path, MAX_SOCKET_RUNTIME_SMALL_FILE_BYTES) {
        Ok(value) if value.trim() == "sdk-envelope-v1" => Ok(true),
        Ok(value) if value.trim() == "argv-v1" => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Ok(_) | Err(_) => Err(SocketRuntimeError::CannotRunAgent),
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "hosted agent loop keeps ordered protocol state transitions auditable"
)]
fn run_agent_envelope_loop(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    request: AgentExecutableRunRequest<'_>,
) -> Result<AgentRunOutcome, SocketRuntimeError> {
    const MAX_CALLS: u8 = 8;
    let mut frames = Vec::new();
    let mut seen = HashSet::new();
    let mut observation = Value::Null;
    let mut approval_delivery_best_effort = false;
    for step in 0..=MAX_CALLS {
        if agent_run_cancelled(&runtime.session_root.join(request.session), request.run_id) {
            return Ok(AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Cancelled,
            });
        }
        let envelope = serde_json::json!({
            "schema": "cortexfs.agent-invocation/v1",
            "run": request.run_id,
            "step": step,
            "input": request.input,
            "history_messages": request.history_messages,
            "tool_context": request.tool_context,
            "observation": observation
        })
        .to_string()
            + "\n";
        if envelope.len() > 1024 * 1024 {
            return Err(SocketRuntimeError::InvalidAgentOutput);
        }
        let step_request = AgentExecutableRunRequest {
            envelope: Some(&envelope),
            step,
            ..request
        };
        let outcome = run_agent_executable_streaming(stream, runtime, step_request)?;
        frames.extend(outcome.frames.clone());
        if outcome.process == AgentProcessOutcome::Cancelled {
            return Ok(AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Cancelled,
            });
        }
        if outcome.process == AgentProcessOutcome::Error {
            let terminal = agent_process_failed_frames(request.run_id, "agent process failed");
            for frame in &terminal {
                deliver_host_frame(stream, frame, approval_delivery_best_effort)?;
            }
            frames.extend(terminal);
            return Ok(AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Error,
            });
        }
        let call = object::executor::call::first_tool_call(&outcome.frames)
            .map_err(|_error| SocketRuntimeError::InvalidAgentOutput)?;
        let Some(call) = call else {
            let done =
                serde_json::json!({"type":"done", "run":request.run_id, "status":"ok"}).to_string();
            deliver_host_frame(stream, &done, approval_delivery_best_effort)?;
            frames.push(done);
            return Ok(AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Success,
            });
        };
        if step == MAX_CALLS || !seen.insert(call.id.clone()) {
            let terminal = agent_process_failed_frames(
                request.run_id,
                if step == MAX_CALLS {
                    "agent tool loop limit exceeded"
                } else {
                    "agent replayed tool call id"
                },
            );
            for frame in &terminal {
                deliver_host_frame(stream, frame, approval_delivery_best_effort)?;
            }
            frames.extend(terminal);
            return Ok(AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Error,
            });
        }
        if agent_run_cancelled(&runtime.session_root.join(request.session), request.run_id) {
            return Ok(AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Cancelled,
            });
        }
        let cancel_dir = runtime.session_root.join(request.session);
        let config = object::executor::exec::AgentToolExecutionConfig {
            agent: runtime.agent_name,
            source: runtime.source_root,
            ctx_root: runtime.ctx_root,
            run: request.run_id,
            session: request.session,
            inherit_control: false,
            cancel: Some((&cancel_dir, request.run_id)),
        };
        let (content, status) =
            match object::executor::exec::prepare_agent_tool_call(&config, &call) {
                Ok(prepared) if prepared.approval() == AgentApprovalMode::Ask => {
                    approval_delivery_best_effort = true;
                    let approval = request_tool_approval(stream, request.run_id, &call)?;
                    let [request_frame, result_frame] = approval.frames;
                    frames.extend([request_frame, result_frame]);
                    if approval.allowed {
                        if agent_run_cancelled(&cancel_dir, request.run_id) {
                            return Ok(AgentRunOutcome {
                                frames,
                                process: AgentProcessOutcome::Cancelled,
                            });
                        }
                        match prepared.execute(&config) {
                            Ok(content) => (content, "ok"),
                            Err(error) => (format!("ERROR: {error}\n"), "error"),
                        }
                    } else {
                        (format!("ERROR: {}\n", approval.reason), "error")
                    }
                }
                Ok(prepared) => match prepared.execute(&config) {
                    Ok(content) => (content, "ok"),
                    Err(error) => (format!("ERROR: {error}\n"), "error"),
                },
                Err(error) => (format!("ERROR: {error}\n"), "error"),
            };
        if agent_run_cancelled(&cancel_dir, request.run_id) {
            return Ok(AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Cancelled,
            });
        }
        let (content, truncated) = normalize_observation(&content);
        let mut output = Vec::new();
        object::executor::output::write_tool_result_event(
            &mut output,
            request.run_id,
            &call,
            &content,
        )
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
        let result = String::from_utf8(output)
            .map_err(|_error| SocketRuntimeError::CannotRunAgent)?
            .trim_end()
            .to_owned();
        deliver_host_frame(stream, &result, approval_delivery_best_effort)?;
        frames.push(result);
        observation = serde_json::json!({
            "tool_call_id": call.id, "name": call.name,
            "status": status, "content": content, "truncated": truncated
        });
    }
    Err(SocketRuntimeError::InvalidAgentOutput)
}

fn deliver_host_frame(
    stream: &mut UnixStream,
    frame: &str,
    best_effort: bool,
) -> Result<(), SocketRuntimeError> {
    match write_socket_frame(stream, frame) {
        Ok(()) => Ok(()),
        Err(_) if best_effort => Ok(()),
        Err(error) => Err(error),
    }
}

struct ToolApproval {
    frames: [String; 2],
    allowed: bool,
    reason: &'static str,
}

fn request_tool_approval(
    stream: &mut UnixStream,
    run: &str,
    call: &object::executor::AgentToolCall,
) -> Result<ToolApproval, SocketRuntimeError> {
    let args = call
        .args
        .iter()
        .map(|arg| arg.to_str().ok_or(SocketRuntimeError::CannotRunAgent))
        .collect::<Result<Vec<_>, _>>()?;
    let request = serde_json::json!({
        "type": "approval_request",
        "run": run,
        "id": call.id,
        "name": call.name,
        "args": args
    })
    .to_string();
    if request.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(SocketRuntimeError::CannotRunAgent);
    }
    let request_delivered = write_socket_frame(stream, &request).is_ok();
    let response = request_delivered
        .then(|| read_socket_request_frame_from_stream(stream))
        .transpose()
        .ok()
        .flatten();
    let decision = response
        .and_then(|line| serde_json::from_str::<Value>(&line).ok())
        .filter(|value| {
            value.as_object().is_some_and(|object| object.len() == 4)
                && value.get("op").and_then(Value::as_str) == Some("approve")
                && value.get("run").and_then(Value::as_str) == Some(run)
                && value.get("id").and_then(Value::as_str) == Some(call.id.as_str())
        })
        .and_then(|value| {
            value
                .get("decision")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    let mut allowed = decision.as_deref() == Some("allow_once");
    let mut reason = if !request_delivered {
        "approval request delivery failed"
    } else if allowed {
        "approved once"
    } else if decision.as_deref() == Some("deny") {
        "tool approval denied"
    } else {
        "invalid or missing tool approval"
    };
    let mut result = approval_result_frame(run, call, allowed, reason);
    if write_socket_frame(stream, &result).is_err() && allowed {
        allowed = false;
        reason = "approval result delivery failed";
        result = approval_result_frame(run, call, false, reason);
    }
    Ok(ToolApproval {
        frames: [request, result],
        allowed,
        reason,
    })
}

fn approval_result_frame(
    run: &str,
    call: &object::executor::AgentToolCall,
    allowed: bool,
    reason: &str,
) -> String {
    serde_json::json!({
        "type": "approval_result",
        "run": run,
        "id": call.id,
        "name": call.name,
        "decision": if allowed { "allow_once" } else { "deny" },
        "reason": reason
    })
    .to_string()
}

fn normalize_observation(value: &str) -> (String, bool) {
    const LIMIT: usize = 16 * 1024;
    if value.len() <= LIMIT {
        return (value.to_owned(), false);
    }
    let marker = "\n[truncated]\n";
    let mut end = LIMIT.saturating_sub(marker.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    (
        format!("{}{marker}", value.get(..end).unwrap_or_default()),
        true,
    )
}

fn record_owned_child_completion(
    runtime: AgentExecutableSocketRuntime<'_>,
    child_session: &str,
    run_id: &str,
    outcome: &AgentRunOutcome,
) -> Result<(), SocketRuntimeError> {
    let Some(control_dir) = (match runtime.execution {
        AgentExecutableSocketExecution::Bwrap {
            control_dir: Some(control_dir),
            ..
        } => Some(control_dir),
        AgentExecutableSocketExecution::Direct
        | AgentExecutableSocketExecution::Bwrap {
            control_dir: None, ..
        } => None,
    }) else {
        return Ok(());
    };
    let view = derive_agent_runtime_view(runtime.source_root, runtime.agent_name)
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    let Some(parent) = view.parent() else {
        return Ok(());
    };
    if view.lifecycle() != ChildLifecycle::Owned {
        return Ok(());
    }
    let (parent_agent, parent_session, _parent_run) =
        parse_exact_parent(parent).ok_or(SocketRuntimeError::CannotRunAgent)?;
    let channel = canonical_owned_child_channel(
        runtime.source_root,
        view.owner(),
        parent_agent,
        parent_session,
        runtime.agent_name,
    )?;
    let metadata =
        fs::symlink_metadata(&channel).map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(SocketRuntimeError::CannotRunAgent);
    }
    let receipt = ChildHandoffReceipt {
        path: channel,
        dev: metadata.dev(),
        ino: metadata.ino(),
    };
    let (status, result) = compact_child_outcome(run_id, outcome);
    let _ = control_dir;
    let finish = finish_child_result_exclusive(
        &receipt,
        runtime.agent_name,
        child_session,
        status,
        &result,
        "",
    );
    finish.map_err(|_error| SocketRuntimeError::CannotRunAgent)
}

fn canonical_owned_child_channel(
    source: &Path,
    child_owner: u32,
    parent_agent: &str,
    parent_session: &str,
    child_agent: &str,
) -> Result<PathBuf, SocketRuntimeError> {
    let parent_view = derive_agent_runtime_view(source, parent_agent)
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    if parent_view.agent_name() != parent_agent || parent_view.owner() != child_owner {
        return Err(SocketRuntimeError::CannotRunAgent);
    }
    Ok(parent_view
        .home()
        .join("session")
        .join(parent_session)
        .join("context/child")
        .join(child_agent))
}

fn parse_exact_parent(value: &str) -> Option<(&str, &str, &str)> {
    let mut fields = value.split_whitespace();
    let agent = fields.next()?.strip_prefix("agent:")?;
    let session = fields.next()?.strip_prefix("session:")?;
    let run = fields.next()?.strip_prefix("run:")?;
    if fields.next().is_some()
        || !is_object_name(agent)
        || !is_object_name(session)
        || !is_object_name(run)
    {
        return None;
    }
    Some((agent, session, run))
}

fn compact_child_outcome(run_id: &str, outcome: &AgentRunOutcome) -> (ChildContextStatus, String) {
    let mut last_message = None;
    let mut deltas = String::new();
    let mut error = None;
    let mut terminal_error = false;
    for frame in &outcome.frames {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            continue;
        };
        if value.get("run").and_then(Value::as_str) != Some(run_id) {
            continue;
        }
        match value.get("type").and_then(Value::as_str) {
            Some("message") if value.get("role").and_then(Value::as_str) == Some("assistant") => {
                last_message = message_event_text(&value);
            }
            Some("delta") => {
                if let Some(text) = value.get("text").and_then(Value::as_str) {
                    deltas.push_str(text);
                }
            }
            Some("error") => {
                error = value
                    .get("message")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
            }
            Some("done") if value.get("status").and_then(Value::as_str) == Some("error") => {
                terminal_error = true;
            }
            _ => {}
        }
    }
    let (status, mut text) = if outcome.process == AgentProcessOutcome::Cancelled {
        (
            ChildContextStatus::Cancelled,
            "child agent run cancelled".to_owned(),
        )
    } else if outcome.process == AgentProcessOutcome::Error || terminal_error {
        (
            ChildContextStatus::Error,
            error.unwrap_or_else(|| "child agent run failed".to_owned()),
        )
    } else {
        (ChildContextStatus::Done, last_message.unwrap_or(deltas))
    };
    if text.len() > 65_536 {
        let mut end = 65_536;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    (status, text)
}

#[expect(
    clippy::too_many_lines,
    reason = "streaming supervision includes capability setup and cleanup"
)]
pub(crate) fn run_agent_executable_streaming(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    request: AgentExecutableRunRequest<'_>,
) -> Result<AgentRunOutcome, SocketRuntimeError> {
    let mut client_connected = true;
    let agent_executable = open_agent_executable_no_follow(runtime.agent_executable)?;
    let control = match runtime.execution {
        AgentExecutableSocketExecution::Direct
        | AgentExecutableSocketExecution::Bwrap {
            control_dir: None, ..
        } => None,
        AgentExecutableSocketExecution::Bwrap {
            control_dir: Some(control_dir),
            ..
        } => {
            let (capability, listener) = runtime::control::RunCapability::create_with_source(
                control_dir,
                runtime.source_root,
                runtime.agent_name,
                request.session,
                request.run_id,
                runtime.identity.uid(),
                runtime.identity.gid(),
            )
            .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
            let environment = capability.environment(Path::new(SOCKET_RUN_CONTROL_PATH));
            Some((capability, listener, environment))
        }
    };
    let command_result = agent_executable_socket_command(
        runtime,
        &agent_executable,
        request,
        control
            .as_ref()
            .map(|entry| (entry.0.socket(), entry.2.as_slice())),
    );
    let (mut command, agent_executable_fd) = match command_result {
        Ok(command) => command,
        Err(error) => {
            if let Some((capability, _listener, _environment)) = control {
                capability
                    .cleanup()
                    .map_err(|_cleanup| SocketRuntimeError::CannotRunAgent)?;
            }
            return Err(error);
        }
    };
    apply_socket_debug_timing_env(&mut command, request.debug);
    apply_agent_identity_to_command(&mut command, runtime.identity);
    command.stderr(Stdio::piped());
    let mut control_server = control.map(|(capability, listener, _environment)| {
        let source_root = runtime.source_root.to_path_buf();
        let ctx_root = runtime.ctx_root.to_path_buf();
        let request_run = request.run_id.to_owned();
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = Arc::clone(&shutdown);
        let (startup_sender, startup_receiver) = mpsc::sync_channel(1);
        let error_sender = startup_sender.clone();
        let join = thread::spawn(move || {
            let result = capability.serve_run_with_handler(
                &listener,
                &server_shutdown,
                &startup_sender,
                || Some(request_run.clone()),
                |request| {
                    agent::createop::create_child_context(
                        &source_root,
                        &ctx_root,
                        &request.agent,
                        &request.session,
                        &request.run,
                        &request.child,
                        Some(&request.child_session),
                        &request.input,
                        "owned",
                    )
                    .map(|(child_session, pid)| runtime::control::CreateChildResult {
                        child: request.child,
                        child_session,
                        pid,
                    })
                    .map_err(|error| match error.errno() {
                        "EACCES" => runtime::control::RunCapabilityError::PeerDenied,
                        "EINVAL" => runtime::control::RunCapabilityError::InvalidFrame,
                        _ => runtime::control::RunCapabilityError::CannotCreate,
                    })
                },
            );
            if let Err(ref error) = result {
                let _ignored = error_sender.try_send(Err(error.clone()));
            }
            let cleanup = capability.cleanup();
            result.and(cleanup)
        });
        RunControlServer {
            shutdown,
            startup: startup_receiver,
            join: Some(join),
        }
    });
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_error) => {
            if let Some(mut server) = control_server {
                let _ignored = server.finish();
            }
            return Err(SocketRuntimeError::CannotRunAgent);
        }
    };
    if let Some(envelope) = request.envelope {
        let mut stdin = child
            .stdin
            .take()
            .ok_or(SocketRuntimeError::CannotRunAgent)?;
        stdin
            .write_all(envelope.as_bytes())
            .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    }
    drop(agent_executable_fd);
    if let Some(server) = control_server.as_ref()
        && !matches!(
            server.startup.recv_timeout(Duration::from_secs(5)),
            Ok(Ok(()))
        )
    {
        terminate_agent_process_group(&mut child);
        let _ignored = child.wait();
        return Err(SocketRuntimeError::CannotRunAgent);
    }
    write_while_connected(&mut client_connected, || {
        write_optional_socket_debug_timing_frame(stream, request.debug, "agent_spawned")
    })?;
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
    let mut yielded_tool_call = None;
    let mut saw_terminal_lifecycle = false;
    loop {
        match stdout_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                if line.trim().is_empty() {
                    continue;
                }
                if is_socket_debug_timing_frame(&line, request.debug) {
                    write_while_connected(&mut client_connected, || {
                        write_socket_frame(stream, &line)
                    })?;
                    continue;
                }
                if !saw_agent_frame {
                    write_while_connected(&mut client_connected, || {
                        write_optional_socket_debug_timing_frame(
                            stream,
                            request.debug,
                            "first_agent_frame",
                        )
                    })?;
                    saw_agent_frame = true;
                }
                if !inspect_event_stream_jsonl(&line).is_ok() {
                    if request.envelope.is_some() {
                        terminate_agent_process_group(&mut child);
                        let _ignored = child.wait();
                        return Err(SocketRuntimeError::InvalidAgentOutput);
                    }
                    if frames.is_empty() {
                        terminate_agent_process_group(&mut child);
                        let _ignored = child.wait();
                        return Err(SocketRuntimeError::InvalidAgentOutput);
                    }
                    let wrapped = agent_plain_text_frame(request.run_id, &line);
                    write_while_connected(&mut client_connected, || {
                        write_socket_frame(stream, &wrapped)
                    })?;
                    frames.push(wrapped);
                    continue;
                }
                let value: Value = serde_json::from_str(&line)
                    .map_err(|_error| SocketRuntimeError::InvalidAgentOutput)?;
                let frame_type = event_type(&line);
                if matches!(
                    frame_type.as_deref(),
                    Some("approval_request" | "approval_result")
                ) || request.envelope.is_some()
                    && matches!(frame_type.as_deref(), Some("start" | "error" | "done"))
                {
                    terminate_agent_process_group(&mut child);
                    let _ignored = child.wait();
                    return Err(SocketRuntimeError::InvalidAgentOutput);
                }
                if agent_frame_has_tool_result(&value) || yielded_tool_call.is_some() {
                    terminate_agent_process_group(&mut child);
                    let _ignored = child.wait();
                    return Err(SocketRuntimeError::InvalidAgentOutput);
                }
                if let Some(tool_call) = object::executor::call::tool_call_from_event_frame(&line)
                    .map_err(|_error| SocketRuntimeError::InvalidAgentOutput)?
                {
                    if saw_terminal_lifecycle {
                        terminate_agent_process_group(&mut child);
                        let _ignored = child.wait();
                        return Err(SocketRuntimeError::InvalidAgentOutput);
                    }
                    yielded_tool_call = Some((line, tool_call));
                    continue;
                }
                saw_terminal_lifecycle |= matches!(frame_type.as_deref(), Some("error" | "done"));
                if frame_type.as_deref() != Some("start") {
                    write_while_connected(&mut client_connected, || {
                        write_socket_frame(stream, &line)
                    })?;
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
    if let Some(server) = control_server.as_mut() {
        server
            .finish()
            .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    }
    if cancelled {
        return Ok(AgentRunOutcome {
            frames,
            process: AgentProcessOutcome::Cancelled,
        });
    }
    if !status.success() && frames.is_empty() {
        let stderr = match stderr_reader.join() {
            Ok(Ok(stderr)) => stderr,
            Ok(Err(_error)) => String::new(),
            Err(_error) => String::new(),
        };
        if request.envelope.is_some() {
            return Ok(AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Error,
            });
        }
        let frames = agent_process_failed_frames(request.run_id, &stderr);
        for frame in &frames {
            write_while_connected(&mut client_connected, || write_socket_frame(stream, frame))?;
        }
        return Ok(AgentRunOutcome {
            frames,
            process: AgentProcessOutcome::Error,
        });
    }
    if let Some((tool_call_frame, tool_call)) = yielded_tool_call {
        if !status.success() {
            return Err(SocketRuntimeError::InvalidAgentOutput);
        }
        write_while_connected(&mut client_connected, || {
            write_socket_frame(stream, &tool_call_frame)
        })?;
        frames.push(tool_call_frame);
        if request.envelope.is_some() {
            return Ok(AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Success,
            });
        }
        let config = object::executor::exec::AgentToolExecutionConfig {
            agent: runtime.agent_name,
            source: runtime.source_root,
            ctx_root: runtime.ctx_root,
            run: request.run_id,
            session: request.session,
            inherit_control: false,
            cancel: None,
        };
        let (result, status) =
            match object::executor::exec::execute_agent_tool_call_with(&config, &tool_call) {
                Ok(result) => (result, "ok"),
                Err(error) => (format!("ERROR: {error}\n"), "error"),
            };
        let mut output = Vec::new();
        object::executor::output::write_tool_result_event(
            &mut output,
            request.run_id,
            &tool_call,
            &result,
        )
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
        let result_frame = String::from_utf8(output)
            .map_err(|_error| SocketRuntimeError::CannotRunAgent)?
            .trim_end()
            .to_owned();
        let done_frame = serde_json::json!({
            "type": "done",
            "run": request.run_id,
            "status": status
        })
        .to_string();
        for frame in [&result_frame, &done_frame] {
            write_while_connected(&mut client_connected, || write_socket_frame(stream, frame))?;
        }
        frames.extend([result_frame, done_frame]);
    }
    Ok(AgentRunOutcome {
        frames,
        process: if status.success() {
            AgentProcessOutcome::Success
        } else {
            AgentProcessOutcome::Error
        },
    })
}

fn agent_frame_has_tool_result(value: &Value) -> bool {
    value.get("type").and_then(Value::as_str) == Some("tool_result")
        || value.get("role").and_then(Value::as_str) == Some("tool")
        || value
            .get("content")
            .and_then(Value::as_array)
            .is_some_and(|parts| {
                parts
                    .iter()
                    .any(|part| part.get("type").and_then(Value::as_str) == Some("tool_result"))
            })
}

fn write_while_connected(
    connected: &mut bool,
    write: impl FnOnce() -> Result<(), SocketRuntimeError>,
) -> Result<(), SocketRuntimeError> {
    if !*connected {
        return Ok(());
    }
    match write() {
        Ok(()) => Ok(()),
        Err(SocketRuntimeError::CannotWriteResponse) => {
            *connected = false;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum AgentProcessOutcome {
    Success,
    Error,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentRunOutcome {
    pub(crate) frames: Vec<String>,
    pub(crate) process: AgentProcessOutcome,
}

pub(crate) fn read_agent_executable_stderr_limited(stderr: impl Read) -> std::io::Result<String> {
    let mut bytes = Vec::new();
    stderr
        .take(MAX_AGENT_EXECUTABLE_STDERR_BYTES.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() > usize::try_from(MAX_AGENT_EXECUTABLE_STDERR_BYTES).unwrap_or(usize::MAX) {
        bytes.truncate(usize::try_from(MAX_AGENT_EXECUTABLE_STDERR_BYTES).unwrap_or(usize::MAX));
    }
    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}

pub(crate) fn agent_process_failed_frames(run_id: &str, stderr: &str) -> Vec<String> {
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

pub(crate) fn agent_plain_text_frame(run_id: &str, text: &str) -> String {
    serde_json::json!({
        "type": "delta",
        "run": run_id,
        "text": text
    })
    .to_string()
}

pub(crate) fn read_agent_executable_frame_line(
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
pub(crate) struct AgentExecutableRunRequest<'a> {
    pub(crate) run_id: &'a str,
    pub(crate) session: &'a str,
    pub(crate) cwd: Option<&'a str>,
    pub(crate) input: &'a str,
    pub(crate) history_messages: &'a str,
    pub(crate) tool_context: &'a str,
    pub(crate) debug: Option<SocketDebugTiming>,
    pub(crate) envelope: Option<&'a str>,
    pub(crate) step: u8,
}

pub(crate) fn agent_tool_context_for_request(cwd: Option<&str>) -> String {
    let mut context = default_agent_tool_context();
    context.push_str("\n\nCurrent request context:\n");
    context.push_str("- Sandbox cwd: ");
    context.push_str(&prompt_quoted(cwd.unwrap_or("/workspace")));
    context.push('\n');
    context.push_str("- Host workspace configuration: determined by agent policy\n");
    context
}

pub(crate) fn prompt_quoted(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_error| "\"\"".to_owned())
}

#[cfg(test)]
mod completion_tests {
    use super::*;
    use std::ffi::OsString;
    use std::net::Shutdown;

    fn approval_call() -> object::executor::AgentToolCall {
        object::executor::AgentToolCall {
            id: "call-1".to_owned(),
            name: "example.echo".to_owned(),
            args: vec![OsString::from("one")],
        }
    }

    #[test]
    fn approval_response_denies_eof_malformed_mismatch_and_timeout()
    -> Result<(), Box<dyn std::error::Error>> {
        for response in [
            "",
            "not-json\n",
            "{\"op\":\"approve\",\"run\":\"wrong\",\"id\":\"call-1\",\"decision\":\"allow_once\"}\n",
            "{\"op\":\"approve\",\"run\":\"run-1\",\"id\":\"wrong\",\"decision\":\"allow_once\"}\n",
            "{\"op\":\"approve\",\"run\":\"run-1\",\"id\":\"call-1\",\"decision\":\"deny\"}\n",
        ] {
            let (mut client, mut server) = UnixStream::pair()?;
            client.write_all(response.as_bytes())?;
            client.shutdown(Shutdown::Write)?;
            let approval = request_tool_approval(&mut server, "run-1", &approval_call())
                .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
            assert!(!approval.allowed, "{response:?}");
        }
        let (_client, mut server) = UnixStream::pair()?;
        server.set_read_timeout(Some(Duration::from_millis(10)))?;
        let approval = request_tool_approval(&mut server, "run-1", &approval_call())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        assert!(!approval.allowed);
        Ok(())
    }

    #[test]
    fn approval_response_denies_replayed_allow_once_for_previous_call()
    -> Result<(), Box<dyn std::error::Error>> {
        let (mut client, mut server) = UnixStream::pair()?;
        client.write_all(
            b"{\"op\":\"approve\",\"run\":\"run-1\",\"id\":\"call-1\",\"decision\":\"allow_once\"}\n\
              {\"op\":\"approve\",\"run\":\"run-1\",\"id\":\"call-1\",\"decision\":\"allow_once\"}\n",
        )?;
        client.shutdown(Shutdown::Write)?;

        let first = request_tool_approval(&mut server, "run-1", &approval_call())
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        assert!(first.allowed);

        let mut second_call = approval_call();
        second_call.id = "call-2".to_owned();
        let replayed = request_tool_approval(&mut server, "run-1", &second_call)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        assert!(!replayed.allowed);
        Ok(())
    }

    fn completion_root(name: &str) -> PathBuf {
        env::temp_dir().join(format!("cfs-{name}-{}", std::process::id()))
    }

    #[test]
    fn owned_child_channel_uses_canonical_parent_home() {
        let root = completion_root("canonical-child-channel");
        let _ignored = fs::remove_dir_all(&root);
        assert!(ensure_v1_reference_tree(&root).is_ok());
        let parent = derive_agent_runtime_view(&root, "coder");
        assert!(parent.is_ok());
        let Ok(parent) = parent else {
            return;
        };
        let channel =
            canonical_owned_child_channel(&root, parent.owner(), "coder", "default", "worker-1");
        assert_eq!(
            channel,
            Ok(parent.home().join("session/default/context/child/worker-1"))
        );
        assert_ne!(
            channel.unwrap_or_default(),
            root.join("agent/coder.d/session/default/context/child/worker-1")
        );
    }

    #[test]
    fn owned_child_channel_rejects_parent_owner_mismatch() {
        let root = completion_root("child-channel-owner-mismatch");
        let _ignored = fs::remove_dir_all(&root);
        assert!(ensure_v1_reference_tree(&root).is_ok());
        let parent = derive_agent_runtime_view(&root, "coder");
        assert!(parent.is_ok());
        let Ok(parent) = parent else {
            return;
        };
        assert_eq!(
            canonical_owned_child_channel(
                &root,
                parent.owner().saturating_add(1),
                "coder",
                "default",
                "worker-1",
            ),
            Err(SocketRuntimeError::CannotRunAgent)
        );
    }

    #[test]
    fn exact_parent_parser_rejects_partial_or_extra_fields() {
        assert_eq!(
            parse_exact_parent("agent:parent session:session run:run"),
            Some(("parent", "session", "run"))
        );
        assert!(parse_exact_parent("agent:parent session:session").is_none());
        assert!(parse_exact_parent("agent:parent session:session run:run extra:x").is_none());
        assert!(parse_exact_parent("session:session agent:parent run:run").is_none());
    }

    #[test]
    fn compact_outcome_prefers_last_assistant_and_excludes_reasoning_and_tools() {
        let frames = vec![
            serde_json::json!({"type":"delta","run":"other","text":"wrong"}).to_string(),
            serde_json::json!({"type":"delta","run":"run","text":"fallback"}).to_string(),
            serde_json::json!({"type":"reasoning_delta","run":"run","text":"secret"}).to_string(),
            serde_json::json!({"type":"tool_call","run":"run","name":"read"}).to_string(),
            serde_json::json!({"type":"message","run":"run","role":"assistant","content":[{"type":"text","text":"first"}]}).to_string(),
            serde_json::json!({"type":"message","run":"run","role":"assistant","content":[{"type":"text","text":"final"}]}).to_string(),
        ];
        let outcome = AgentRunOutcome {
            frames,
            process: AgentProcessOutcome::Success,
        };
        assert_eq!(
            compact_child_outcome("run", &outcome),
            (ChildContextStatus::Done, "final".to_owned())
        );
    }

    #[test]
    fn compact_outcome_uses_stable_error_and_utf8_safe_limit() {
        let frames = vec![
            serde_json::json!({
                "type":"done", "run":"run", "status":"error"
            })
            .to_string(),
        ];
        assert_eq!(
            compact_child_outcome(
                "run",
                &AgentRunOutcome {
                    frames,
                    process: AgentProcessOutcome::Success
                }
            ),
            (
                ChildContextStatus::Error,
                "child agent run failed".to_owned()
            )
        );
        let long = "界".repeat(30_000);
        let frames = vec![serde_json::json!({"type":"delta","run":"run","text":long}).to_string()];
        let (_, output) = compact_child_outcome(
            "run",
            &AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Success,
            },
        );
        assert!(output.len() <= 65_536);
        assert!(output.is_char_boundary(output.len()));
    }

    #[test]
    fn process_error_and_cancel_override_assistant_frames() {
        let frame = serde_json::json!({"type":"message","run":"run","role":"assistant","content":[{"type":"text","text":"partial"}]}).to_string();
        let error = AgentRunOutcome {
            frames: vec![frame.clone()],
            process: AgentProcessOutcome::Error,
        };
        assert_eq!(
            compact_child_outcome("run", &error).0,
            ChildContextStatus::Error
        );
        let cancelled = AgentRunOutcome {
            frames: vec![frame],
            process: AgentProcessOutcome::Cancelled,
        };
        assert_eq!(
            compact_child_outcome("run", &cancelled).0,
            ChildContextStatus::Cancelled
        );
    }
}
