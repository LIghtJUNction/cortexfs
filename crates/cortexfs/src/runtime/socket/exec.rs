use super::*;
use cortexfs_runtime_client::interaction::InteractionOrigin;
use std::ffi::OsString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

mod approval;
mod delivery;

use approval::request_tool_approval;

#[cfg(test)]
thread_local! {
    static CAPTURE_SOCKET_CHILD_WINDOW: std::cell::RefCell<Vec<Option<u32>>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static CAPTURE_SOCKET_CHILD_WINDOW_ENABLED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

struct RunControlServer {
    shutdown: Arc<AtomicBool>,
    startup: mpsc::Receiver<Result<(), runtime::control::RunCapabilityError>>,
    startup_confirmed: bool,
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

pub(crate) struct StartedRunControl {
    tool: object::executor::exec::AgentToolControl,
    environment: [(String, String); 1],
    server: RunControlServer,
}

impl StartedRunControl {
    fn launch_gate(&self) -> Result<runtime::control::LaunchGate, SocketRuntimeError> {
        self.tool
            .launch_gate()
            .map_err(|_error| SocketRuntimeError::CannotRunAgent)
    }

    fn await_startup(&mut self) -> Result<(), SocketRuntimeError> {
        if self.server.startup_confirmed {
            return Ok(());
        }
        match self.server.startup.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {
                self.server.startup_confirmed = true;
                Ok(())
            }
            Ok(Err(_)) | Err(_) => Err(SocketRuntimeError::CannotRunAgent),
        }
    }

    fn finish(&mut self) -> Result<(), SocketRuntimeError> {
        self.server
            .finish()
            .map_err(|_error| SocketRuntimeError::CannotRunAgent)
    }
}

fn start_run_control(
    runtime: AgentExecutableSocketRuntime<'_>,
    session: &str,
    run: &str,
) -> Result<Option<StartedRunControl>, SocketRuntimeError> {
    let Some(control_dir) = runtime.environment.control_dir() else {
        return Ok(None);
    };
    let (capability, listener) = runtime::control::RunCapability::create_with_source(
        control_dir,
        runtime.source_root,
        runtime.agent_name,
        session,
        run,
        runtime.identity.uid(),
        runtime.identity.gid(),
    )
    .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    let capability = Arc::new(capability);
    let target = Path::new(SOCKET_RUN_CONTROL_PATH);
    let environment = runtime::control::RunCapability::environment(target);
    let tool = object::executor::exec::AgentToolControl::new(
        capability.socket().to_path_buf(),
        target.to_path_buf(),
        Arc::clone(&capability),
    );
    let source_root = runtime.source_root.to_path_buf();
    let ctx_root = runtime.ctx_root.to_path_buf();
    let request_run = run.to_owned();
    let shutdown = Arc::new(AtomicBool::new(false));
    let server_shutdown = Arc::clone(&shutdown);
    let (startup_sender, startup) = mpsc::sync_channel(1);
    let error_sender = startup_sender.clone();
    let join = thread::spawn(move || {
        let result = capability.serve_run_with_handler(
            &listener,
            &server_shutdown,
            &startup_sender,
            || Some(request_run.clone()),
            |request| create_socket_child(&source_root, &ctx_root, request),
            |request| update_socket_prompt(&source_root, &request),
        );
        if let Err(ref error) = result {
            let _ignored = error_sender.try_send(Err(error.clone()));
        }
        let cleanup = capability.cleanup();
        result.and(cleanup)
    });
    Ok(Some(StartedRunControl {
        tool,
        environment,
        server: RunControlServer {
            shutdown,
            startup,
            startup_confirmed: false,
            join: Some(join),
        },
    }))
}

fn handle_agent_stop_request(
    stream: &mut UnixStream,
    runtime_agent: &str,
    stop: Option<&dyn AgentStopHandler>,
    peer_uid: u32,
    agent: &str,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    if agent != runtime_agent {
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
    Ok(response)
}

fn create_socket_child(
    source_root: &Path,
    ctx_root: &Path,
    request: runtime::control::CreateChildRequest,
) -> Result<runtime::control::CreateChildResult, runtime::control::RunCapabilityError> {
    #[cfg(test)]
    if CAPTURE_SOCKET_CHILD_WINDOW_ENABLED.with(std::cell::Cell::get) {
        CAPTURE_SOCKET_CHILD_WINDOW.with(|capture| capture.borrow_mut().push(request.window));
        return Err(runtime::control::RunCapabilityError::CannotCreate);
    }
    agent::createop::create_child_context(
        source_root,
        ctx_root,
        &request.agent,
        &request.session,
        &request.run,
        &request.child,
        Some(&request.child_session),
        request.path.as_deref(),
        request.window,
        &request.input,
        &request.life,
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
}

/// Applies one authorized self prompt-control update to the backing source.
///
/// The capability socket already binds the request to the running agent, so
/// this only revalidates the authority-free control name and content before
/// atomically replacing `agent/<name>.d/<control>`.
fn update_socket_prompt(
    source_root: &Path,
    request: &runtime::control::UpdatePromptRequest,
) -> Result<(), runtime::control::RunCapabilityError> {
    if !is_object_name(&request.agent)
        || !cortexfs_runtime_client::is_agent_prompt_control(&request.control)
    {
        return Err(runtime::control::RunCapabilityError::InvalidFrame);
    }
    validate_agent_bootstrap_control_content(&request.control, &request.content)
        .map_err(|_error| runtime::control::RunCapabilityError::InvalidFrame)?;
    let control_dir = cortexfs_paths::agent_control_path(source_root, &request.agent);
    open_plain_directory(&control_dir)
        .map_err(|_error| runtime::control::RunCapabilityError::CannotWrite)?;
    // 0o644 matches bootstrap-created prompt controls, so a self-updated
    // control keeps the same mode as its peers instead of drifting to 0o600.
    atomic_replace_text_with_mode(&control_dir.join(&request.control), &request.content, 0o644)
        .map_err(|_error| runtime::control::RunCapabilityError::CannotWrite)
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
    if let Some(result) = handle_agent_immediate_request(stream, runtime, stop, peer_uid, &request)
    {
        return result;
    }
    let SocketRequest::Send {
        ref id,
        ref session,
        scope,
        ref cwd,
        ref input,
        ref event,
        ref origin,
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
    let recorder_outcome = handle_socket_send(
        runtime.session_root,
        runtime.default_cwd,
        runtime.model,
        &request,
        preparation.as_ref(),
    )?;
    let recorder_response = match recorder_outcome {
        SocketSendOutcome::Recorded(response) => response,
        SocketSendOutcome::Replayed(response) => {
            write_socket_runtime_response(stream, &response)?;
            return Ok(response);
        }
    };
    let start = recorder_response
        .frames()
        .first()
        .and_then(|frame| serde_json::from_str::<Value>(frame).ok())
        .ok_or(SocketRuntimeError::CannotRunAgent)?;
    let run_id = start
        .get("run")
        .and_then(Value::as_str)
        .ok_or(SocketRuntimeError::CannotRunAgent)?
        .to_owned();
    let tool_context = agent_tool_context_for_request(cwd.as_deref())?;
    let channel = channel_context_for_request(&runtime, origin.as_ref())?;
    let mut client_connected = true;
    write_while_connected(&mut client_connected, || {
        write_optional_socket_debug_timing_frame(stream, debug, "socket_send_received")
    })?;
    let debug = debug.map(SocketDebugTiming::with_request_baseline);
    write_while_connected(&mut client_connected, || {
        write_optional_socket_debug_timing_frame(stream, debug, "history_collected")
    })?;
    write_while_connected(&mut client_connected, || {
        write_socket_runtime_response(stream, &recorder_response)
    })?;
    write_while_connected(&mut client_connected, || {
        write_optional_socket_debug_timing_frame(stream, debug, "session_recorded")
    })?;

    let run_request = AgentExecutableRunRequest {
        request_id: id,
        run_id: &run_id,
        cancellation_id: &run_id,
        session,
        cwd: cwd.as_deref(),
        input,
        event: event.as_ref(),
        origin: origin.as_ref(),
        channel: channel.as_ref(),
        history_messages: &history_messages,
        tool_context: &tool_context,
        debug,
    };
    let record_dir = (scope != SocketSessionScope::Temp).then_some(session_dir.as_path());
    let agent_outcome = run_agent_request(stream, runtime, run_request, record_dir)?;
    record_owned_child_completion(runtime, session, &run_id, &agent_outcome)?;
    let agent_process = agent_outcome.process;
    let agent_frames = agent_outcome.frames;
    if scope != SocketSessionScope::Temp {
        let batch = AgentFrameBatch::parse(&run_id, &agent_frames);
        if agent_process != AgentProcessOutcome::Cancelled
            && !batch
                .settle(&session_dir, &run_id)
                .map_err(SocketRuntimeError::Record)?
        {
            return Err(SocketRuntimeError::Record(
                SocketSessionRecordError::CannotRecord,
            ));
        }
    }
    delivery::deliver_terminal_batch(stream, &agent_frames, agent_process)?;

    let mut frames = recorder_response.frames().to_vec();
    frames.extend(agent_frames);
    Ok(SocketRuntimeResponse::new(frames))
}

fn channel_context_for_request(
    runtime: &AgentExecutableSocketRuntime<'_>,
    origin: Option<&InteractionOrigin>,
) -> Result<Option<runtime::channelenv::ChannelRuntimeContext>, SocketRuntimeError> {
    let base = runtime::channelenv::base_tool_path(
        runtime.env,
        runtime.source_root,
        runtime.identity.uid(),
    );
    let context =
        runtime::channelenv::resolve(runtime.source_root, runtime.identity.uid(), &base, origin)
            .map_err(SocketRuntimeError::ChannelContext)?;
    Ok(context)
}

fn handle_agent_immediate_request(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    stop: Option<&dyn AgentStopHandler>,
    peer_uid: u32,
    request: &SocketRequest,
) -> Option<Result<SocketRuntimeResponse, SocketRuntimeError>> {
    match *request {
        SocketRequest::Stop { ref agent } => Some(handle_agent_stop_request(
            stream,
            runtime.agent_name,
            stop,
            peer_uid,
            agent,
        )),
        SocketRequest::Tsh {
            ref id,
            ref session,
            ref args,
        } => Some(handle_agent_tsh_request(stream, runtime, id, session, args)),
        _ => None,
    }
}

fn handle_agent_tsh_request(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    id: &str,
    session: &str,
    args: &[String],
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let preparation = prepare_owned_durable_session(
        runtime.session_root,
        session,
        runtime.default_cwd,
        runtime.model,
        SocketSessionScope::Private,
        runtime.identity.uid(),
        runtime.identity.gid(),
    )
    .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    let input = serde_json::to_string(args).map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    let send = SocketRequest::Send {
        id: id.to_owned(),
        session: session.to_owned(),
        scope: SocketSessionScope::Private,
        cwd: None,
        workspace: None,
        input: format!(":tsh {input}"),
        event: None,
        origin: None,
    };
    let recorder = match handle_socket_send(
        runtime.session_root,
        runtime.default_cwd,
        runtime.model,
        &send,
        Some(&preparation),
    )? {
        SocketSendOutcome::Replayed(response) => {
            let response = replayed_run_response(&runtime.session_root.join(session), &response);
            write_socket_runtime_response(stream, &response)?;
            return Ok(response);
        }
        SocketSendOutcome::Recorded(response) => response,
    };
    let run = response_run(&recorder).ok_or(SocketRuntimeError::CannotRunAgent)?;
    let call = object::executor::AgentToolCall {
        id: "tsh".to_owned(),
        name: "tsh".to_owned(),
        args: args.iter().map(OsString::from).collect(),
    };
    let mut control =
        start_run_control(runtime, session, &run)?.ok_or(SocketRuntimeError::CannotRunAgent)?;
    let config = object::executor::exec::AgentToolExecutionConfig {
        agent: runtime.agent_name,
        source: runtime.source_root,
        ctx_root: runtime.ctx_root,
        run: &run,
        session,
        control: Some(control.tool.clone()),
        cancel: None,
        tool_path: None,
        channel: None,
    };
    let mut bytes = Vec::new();
    object::executor::output::write_tool_call_event(&mut bytes, &run, &call)
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    let prepared = object::executor::exec::prepare_agent_tool_call(&config, &call);
    let approval = prepared
        .as_ref()
        .ok()
        .filter(|call| call.approval() == AgentApprovalMode::Ask)
        .map(|_| request_tool_approval(stream, id, &run, &call))
        .transpose()?;
    let result = prepared.and_then(|call| match approval.as_ref() {
        Some(entry) if !entry.allowed => Err(object::executor::ExecError::new(entry.reason)),
        _ => call.execute(&config),
    });
    control.finish()?;
    let content = result
        .as_ref()
        .map_or_else(|error| error.message(), String::as_str);
    object::executor::output::write_tool_result_event(&mut bytes, &run, &call, content)
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    let mut frames = String::from_utf8(bytes)
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if let Some(approval) = approval {
        frames.extend(approval.frames);
    }
    if result.is_err() {
        frames.extend(agent_process_failed_frames(&run, content));
    } else {
        frames.push(serde_json::json!({"type":"done", "run":run, "status":"ok"}).to_string());
    }
    let session_dir = runtime.session_root.join(session);
    record_tool_execution_result_to_session(&session_dir, &run, &call.id, &call.name, content)
        .map_err(SocketRuntimeError::Record)?;
    let batch = AgentFrameBatch::parse(&run, &frames);
    if !batch
        .settle(&session_dir, &run)
        .map_err(SocketRuntimeError::Record)?
    {
        return Err(SocketRuntimeError::Record(
            SocketSessionRecordError::CannotRecord,
        ));
    }
    let mut all = recorder.frames().to_vec();
    all.extend(frames);
    let response = SocketRuntimeResponse::new(all);
    write_socket_runtime_response(stream, &response)?;
    Ok(response)
}

fn replayed_run_response(
    session_dir: &Path,
    response: &SocketRuntimeResponse,
) -> SocketRuntimeResponse {
    let Some(run) = response_run(response) else {
        return response.clone();
    };
    let Ok(events) = columnar::read_text(
        session_dir,
        columnar::Stream::Events,
        MAX_SOCKET_RUNTIME_EVENTS_BYTES,
    ) else {
        return response.clone();
    };
    let frames = events
        .lines()
        .filter(|line| {
            serde_json::from_str::<Value>(line)
                .is_ok_and(|value| value.get("run").and_then(Value::as_str) == Some(run.as_str()))
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if frames.is_empty() {
        response.clone()
    } else {
        SocketRuntimeResponse::new(frames)
    }
}

fn response_run(response: &SocketRuntimeResponse) -> Option<String> {
    let value = serde_json::from_str::<Value>(response.frames().first()?).ok()?;
    value.get("run")?.as_str().map(str::to_owned)
}

pub(crate) fn run_agent_request(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    request: AgentExecutableRunRequest<'_>,
    record_dir: Option<&Path>,
) -> Result<AgentRunOutcome, SocketRuntimeError> {
    let outcome = run_agent_envelope_loop(stream, runtime, request, record_dir);
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(SocketRuntimeError::CannotRunAgent) => {
            let frames = agent_process_failed_frames(request.run_id, "agent request failed");
            AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Error,
            }
        }
        Err(SocketRuntimeError::InvalidAgentOutput) => AgentRunOutcome {
            frames: agent_invalid_output_frames(request.run_id),
            process: AgentProcessOutcome::Error,
        },
        Err(error) => return Err(error),
    };
    canonicalize_agent_outcome(request.run_id, outcome)
}

fn canonicalize_agent_outcome(
    run_id: &str,
    mut outcome: AgentRunOutcome,
) -> Result<AgentRunOutcome, SocketRuntimeError> {
    let mut done_status = None;
    let mut terminal_error = None;
    outcome.frames.retain(|frame| {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            return true;
        };
        if value.get("run").and_then(Value::as_str) == Some(run_id) {
            match value.get("type").and_then(Value::as_str) {
                Some("done") => {
                    done_status = value
                        .get("status")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    return false;
                }
                Some("error")
                    if value.get("recoverable").and_then(Value::as_bool) != Some(true) =>
                {
                    terminal_error = Some(frame.clone());
                    return false;
                }
                _ => {}
            }
        }
        true
    });
    if outcome.process == AgentProcessOutcome::Cancelled {
        return Ok(outcome);
    }
    let status = match (
        outcome.process,
        terminal_error.is_some(),
        done_status.as_deref(),
    ) {
        (AgentProcessOutcome::Success, false, None | Some("ok")) => "ok",
        (_, true, _)
        | (AgentProcessOutcome::Success, false, Some("error"))
        | (AgentProcessOutcome::Error, false, _) => {
            outcome.process = AgentProcessOutcome::Error;
            "error"
        }
        (AgentProcessOutcome::Success, false, Some(_)) => {
            return Err(SocketRuntimeError::InvalidAgentOutput);
        }
        (AgentProcessOutcome::Cancelled, _, _) => return Ok(outcome),
    };
    if status == "error" {
        let error = terminal_error.unwrap_or_else(|| {
            serde_json::json!({
                "type":"error", "run":run_id, "code":"EIO", "message":"agent process failed"
            })
            .to_string()
        });
        outcome.frames.push(error);
    }
    let done = serde_json::json!({"type":"done", "run":run_id, "status":status}).to_string();
    outcome.frames.push(done);
    Ok(outcome)
}

fn record_owned_child_completion(
    runtime: AgentExecutableSocketRuntime<'_>,
    child_session: &str,
    run_id: &str,
    outcome: &AgentRunOutcome,
) -> Result<(), SocketRuntimeError> {
    let Some(control_dir) = runtime.environment.control_dir() else {
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
    let (parent_agent, parent_session) = match classify_parent_reference(parent) {
        ParentReference::Static { .. } => return Ok(()),
        ParentReference::Delegated { agent, session, .. } => (agent, session),
        ParentReference::Malformed => return Err(SocketRuntimeError::CannotRunAgent),
    };
    let channel = canonical_owned_child_channel(
        runtime.source_root,
        view.owner(),
        parent_agent,
        parent_session,
        runtime.agent_name,
    )?;
    let receipt =
        child_handoff_receipt(&channel).map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ParentReference<'a> {
    Static {
        agent: &'a str,
    },
    Delegated {
        agent: &'a str,
        session: &'a str,
        run: &'a str,
    },
    Malformed,
}

fn classify_parent_reference(value: &str) -> ParentReference<'_> {
    if let Some(agent) = value.strip_prefix("agent:")
        && !agent.contains(char::is_whitespace)
        && is_object_name(agent)
    {
        return ParentReference::Static { agent };
    }
    parse_exact_parent(value).map_or(ParentReference::Malformed, |(agent, session, run)| {
        ParentReference::Delegated {
            agent,
            session,
            run,
        }
    })
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
        || value != format!("agent:{agent} session:{session} run:{run}")
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
    reason = "hosted agent loop keeps ordered protocol state transitions auditable"
)]
fn run_agent_envelope_loop(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    request: AgentExecutableRunRequest<'_>,
    record_dir: Option<&Path>,
) -> Result<AgentRunOutcome, SocketRuntimeError> {
    if provider_network_denied(runtime) {
        return Ok(agent_error_outcome(
            Vec::new(),
            agent_provider_network_denied_frames(request.run_id),
        ));
    }
    open_agent_executable_no_follow(runtime.agent_executable)?;
    let provider_egress = create_run_provider_egress(runtime, request)?;
    let mut control = start_run_control(runtime, request.session, request.run_id)?;
    let result = run_agent_envelope_loop_with_control(
        stream,
        runtime,
        request,
        record_dir,
        control.as_mut(),
        provider_egress.as_ref(),
    );
    if let Some(control) = control.as_mut() {
        control.finish()?;
    }
    result
}

fn provider_network_denied(runtime: AgentExecutableSocketRuntime<'_>) -> bool {
    !runtime.network_allowed
        && runtime.environment.is_sandboxed()
        && runtime.model.is_some_and(|model| {
            runtime::egress::is_provider_model(runtime.ctx_root, model)
                .is_ok_and(|provider| provider)
        })
}

#[expect(
    clippy::too_many_lines,
    reason = "hosted agent loop keeps ordered protocol state transitions auditable"
)]
fn run_agent_envelope_loop_with_control(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    request: AgentExecutableRunRequest<'_>,
    record_dir: Option<&Path>,
    mut control: Option<&mut StartedRunControl>,
    provider_egress: Option<&runtime::egress::ProviderEgress>,
) -> Result<AgentRunOutcome, SocketRuntimeError> {
    let max_steps = runtime
        .env
        .iter()
        .find_map(|(name, value)| (name == "CTX_AGENT_STEPS").then_some(value))
        .and_then(|value| value.parse().ok())
        .filter(|value: &u8| *value > 0)
        .unwrap_or(abi::constants::DEFAULT_AGENT_STEPS);
    let mut frames = Vec::new();
    let mut seen = HashSet::new();
    let mut observation = Value::Null;
    let mut tool_context = request.tool_context.to_owned();
    for step in 0..=max_steps {
        if agent_run_cancelled(
            &runtime.session_root.join(request.session),
            request.cancellation_id,
        ) {
            return Ok(AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Cancelled,
            });
        }
        let observation_text = observation.to_string();
        let context_revision = runtime::observation::context_revision(
            request.history_messages,
            &tool_context,
            &observation_text,
        );
        if let Some(record_dir) = record_dir {
            set_session_runtime_observation(
                record_dir,
                request.run_id,
                step,
                "agent",
                None,
                Some(&context_revision),
            )
            .map_err(SocketRuntimeError::Record)?;
        }
        let mut envelope = serde_json::json!({
            "schema": "cortexfs.agent-invocation/v1",
            "run": request.run_id,
            "step": step,
            "input": request.input,
            "history_messages": request.history_messages,
            "tool_context": tool_context,
            "observation": observation
        });
        if let Some(event) = request.event
            && let Some(object) = envelope.as_object_mut()
        {
            object.insert("event".to_owned(), event.clone());
        }
        if let Some(origin) = request.origin
            && let Some(object) = envelope.as_object_mut()
        {
            object.insert(
                "origin".to_owned(),
                serde_json::to_value(origin)
                    .map_err(|_error| SocketRuntimeError::CannotRunAgent)?,
            );
        }
        let envelope = envelope.to_string() + "\n";
        if envelope.len() > 1024 * 1024 {
            return Ok(agent_error_outcome(
                frames,
                agent_invalid_output_frames(request.run_id),
            ));
        }
        let outcome = match run_agent_executable_streaming(
            stream,
            runtime,
            request,
            &envelope,
            step,
            control.as_deref_mut(),
            provider_egress,
        ) {
            Ok(outcome) => outcome,
            Err(SocketRuntimeError::CannotRunAgent) => {
                return Ok(agent_error_outcome(
                    frames,
                    agent_process_failed_frames(request.run_id, "agent request failed"),
                ));
            }
            Err(SocketRuntimeError::InvalidAgentOutput) => {
                return Ok(agent_error_outcome(
                    frames,
                    agent_invalid_output_frames(request.run_id),
                ));
            }
            Err(error) => return Err(error),
        };
        let process = outcome.process;
        let new_frames = outcome.frames;
        let call = if process == AgentProcessOutcome::Success {
            match object::executor::call::first_tool_call(&new_frames) {
                Ok(call) => call,
                Err(_error) => {
                    frames.extend(new_frames);
                    return Ok(agent_error_outcome(
                        frames,
                        agent_invalid_output_frames(request.run_id),
                    ));
                }
            }
        } else {
            None
        };
        let process_message = (process == AgentProcessOutcome::Error)
            .then(|| terminal_error_message(request.run_id, &new_frames))
            .flatten();
        frames.extend(new_frames);
        if process == AgentProcessOutcome::Cancelled {
            return Ok(AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Cancelled,
            });
        }
        if process == AgentProcessOutcome::Error {
            return Ok(agent_error_outcome(
                frames,
                agent_process_failed_frames(
                    request.run_id,
                    process_message.as_deref().unwrap_or("agent process failed"),
                ),
            ));
        }
        let Some(call) = call else {
            let done =
                serde_json::json!({"type":"done", "run":request.run_id, "status":"ok"}).to_string();
            frames.push(done);
            return Ok(AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Success,
            });
        };
        if let Some(record_dir) = record_dir {
            set_session_runtime_observation(
                record_dir,
                request.run_id,
                step,
                "tool_call",
                Some(&call.name),
                Some(&context_revision),
            )
            .map_err(SocketRuntimeError::Record)?;
        }
        if step == max_steps || !seen.insert(call.id.clone()) {
            return Ok(agent_error_outcome(
                frames,
                agent_process_failed_frames(
                    request.run_id,
                    if step == max_steps {
                        "agent tool loop limit exceeded"
                    } else {
                        "agent replayed tool call id"
                    },
                ),
            ));
        }
        if agent_run_cancelled(
            &runtime.session_root.join(request.session),
            request.cancellation_id,
        ) {
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
            control: control.as_ref().map(|entry| entry.tool.clone()),
            cancel: Some((&cancel_dir, request.cancellation_id)),
            tool_path: request
                .channel
                .map(runtime::channelenv::ChannelRuntimeContext::tool_path),
            channel: request.channel,
        };
        let (content, status) =
            match object::executor::exec::prepare_agent_tool_call(&config, &call) {
                Ok(prepared) if prepared.approval() == AgentApprovalMode::Ask => {
                    let approval =
                        request_tool_approval(stream, request.request_id, request.run_id, &call)?;
                    if let Some(session_dir) = record_dir {
                        record_tool_approval_frames(session_dir, &approval.frames)
                            .map_err(SocketRuntimeError::Record)?;
                    }
                    let [request_frame, result_frame] = approval.frames;
                    frames.extend([request_frame, result_frame]);
                    if approval.allowed {
                        if agent_run_cancelled(&cancel_dir, request.cancellation_id) {
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
        if agent_run_cancelled(&cancel_dir, request.cancellation_id) {
            return Ok(AgentRunOutcome {
                frames,
                process: AgentProcessOutcome::Cancelled,
            });
        }
        let (content, truncated) = delivery::normalize_observation(&content);
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
        if let Some(session_dir) = record_dir {
            record_tool_execution_result_to_session(
                session_dir,
                request.run_id,
                &call.id,
                &call.name,
                &content,
            )
            .map_err(SocketRuntimeError::Record)?;
        }
        delivery::deliver_host_frame(stream, &result)?;
        frames.push(result);
        observation = serde_json::json!({
            "tool_call_id": call.id, "name": call.name,
            "status": status, "content": content, "truncated": truncated
        });
        tool_context = continuation_tool_context(request.tool_context, &call, status);
    }
    Err(SocketRuntimeError::InvalidAgentOutput)
}

fn continuation_tool_context(
    base: &str,
    call: &object::executor::AgentToolCall,
    status: &str,
) -> String {
    let mut context = base.to_owned();
    if !context.trim().is_empty() {
        context.push_str("\n\n");
    }
    context.push_str("Latest host tool completion:\n- tool: ");
    context.push_str(&call.name);
    context.push_str("\n- args: ");
    let args = object::executor::tool_call_args_strings(call);
    context.push_str(&serde_json::to_string(&args).unwrap_or_else(|_error| "[]".to_owned()));
    context.push_str("\n- status: ");
    context.push_str(status);
    context.push('\n');
    if status == "ok" {
        context.push_str(
            "The host completed this exact call. Use the authoritative result below and answer the original user; do not repeat this call.",
        );
    } else {
        context.push_str(
            "The host rejected or failed this call. Inspect the error before choosing a different action; do not blindly repeat it.",
        );
    }
    object::executor::trim_tool_context_to_limit(&mut context);
    context
}

fn create_run_provider_egress(
    runtime: AgentExecutableSocketRuntime<'_>,
    request: AgentExecutableRunRequest<'_>,
) -> Result<Option<runtime::egress::ProviderEgress>, SocketRuntimeError> {
    if !runtime.environment.is_sandboxed() || !runtime.network_allowed {
        return Ok(None);
    }
    let provider_model = runtime
        .model
        .map(|model| runtime::egress::is_provider_model(runtime.ctx_root, model))
        .transpose()
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?
        .unwrap_or(false);
    if !provider_model {
        return Ok(None);
    }
    let control_dir = runtime
        .environment
        .control_dir()
        .ok_or(SocketRuntimeError::CannotRunAgent)?;
    let plan = runtime::egress::ProviderEgressPlan::from_controls(
        runtime.ctx_root,
        runtime.model.ok_or(SocketRuntimeError::CannotRunAgent)?,
        runtime.env,
        request.run_id,
    )
    .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    runtime::egress::ProviderEgress::create(
        control_dir,
        plan,
        runtime.identity.uid(),
        runtime.identity.gid(),
    )
    .map(Some)
    .map_err(|_error| SocketRuntimeError::CannotRunAgent)
}

#[expect(
    clippy::too_many_lines,
    clippy::too_many_arguments,
    reason = "streaming supervision keeps its socket, runtime, request, lifecycle, and egress boundaries explicit"
)]
pub(crate) fn run_agent_executable_streaming(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    request: AgentExecutableRunRequest<'_>,
    envelope: &str,
    step: u8,
    control: Option<&mut StartedRunControl>,
    provider_egress: Option<&runtime::egress::ProviderEgress>,
) -> Result<AgentRunOutcome, SocketRuntimeError> {
    let mut client_connected = true;
    let agent_executable = open_agent_executable_no_follow(runtime.agent_executable)?;
    let mut gate = control
        .as_ref()
        .map(|entry| entry.launch_gate())
        .transpose()?;
    let command_control = match (control.as_ref(), gate.as_ref()) {
        (Some(entry), Some(gate)) => Some((
            entry.tool.source.as_path(),
            entry.environment.as_slice(),
            gate.block_fd(),
        )),
        (None, None) => None,
        _ => return Err(SocketRuntimeError::CannotRunAgent),
    };
    let command_result = agent_executable_socket_command(
        runtime,
        &agent_executable,
        request,
        step,
        command_control,
        provider_egress.map(runtime::egress::ProviderEgress::host_dir),
        provider_egress.map(runtime::egress::ProviderEgress::client_token),
    );
    let (mut command, agent_executable_fd) = command_result?;
    apply_socket_debug_timing_env(&mut command, request.debug);
    command.stderr(Stdio::piped());
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(_error) => return Err(SocketRuntimeError::CannotRunAgent),
    };
    if let Some(gate) = gate.as_mut()
        && gate.register_and_release(child.id()).is_err()
    {
        terminate_agent_process_group(&mut child);
        let _ignored = child.wait();
        return Err(SocketRuntimeError::CannotRunAgent);
    }
    let mut stdin = child
        .stdin
        .take()
        .ok_or(SocketRuntimeError::CannotRunAgent)?;
    // A fast-exiting executable can close stdin before the envelope arrives;
    // its emitted frames and exit status stay the authoritative outcome.
    if let Err(error) = stdin.write_all(envelope.as_bytes())
        && error.kind() != std::io::ErrorKind::BrokenPipe
    {
        terminate_agent_process_group(&mut child);
        let _ignored = child.wait();
        return Err(SocketRuntimeError::CannotRunAgent);
    }
    drop(stdin);
    drop(agent_executable_fd);
    if let Some(control) = control
        && control.await_startup().is_err()
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
    let mut frame_bytes = 0usize;
    let session_dir = runtime.session_root.join(request.session);
    let mut cancelled = false;
    let mut saw_agent_frame = false;
    let mut yielded_tool_call = None;
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
                if !inspect_event_stream_jsonl(&format!("{line}\n")).is_ok() {
                    terminate_agent_process_group(&mut child);
                    let _ignored = child.wait();
                    return Err(SocketRuntimeError::InvalidAgentOutput);
                }
                let value: Value = serde_json::from_str(&line)
                    .map_err(|_error| SocketRuntimeError::InvalidAgentOutput)?;
                let frame_type = event_type(&line);
                let recoverable_error =
                    value.get("recoverable").and_then(Value::as_bool) == Some(true);
                if matches!(
                    frame_type.as_deref(),
                    Some("approval_request" | "approval_result" | "start" | "done")
                ) || frame_type.as_deref() == Some("error") && !recoverable_error
                {
                    terminate_agent_process_group(&mut child);
                    let _ignored = child.wait();
                    return Err(SocketRuntimeError::InvalidAgentOutput);
                }
                if agent_frame_has_tool_result(&value) {
                    terminate_agent_process_group(&mut child);
                    let _ignored = child.wait();
                    return Err(SocketRuntimeError::InvalidAgentOutput);
                }
                let next_frame_bytes = frame_bytes.saturating_add(line.len());
                if yielded_tool_call.is_some() || next_frame_bytes > MAX_SOCKET_RUNTIME_OUTPUT_BYTES
                {
                    terminate_agent_process_group(&mut child);
                    let _ignored = child.wait();
                    return Err(SocketRuntimeError::InvalidAgentOutput);
                }
                if object::executor::call::tool_call_from_event_frame(&line)
                    .map_err(|_error| SocketRuntimeError::InvalidAgentOutput)?
                    .is_some()
                {
                    yielded_tool_call = Some(line);
                    continue;
                }
                write_while_connected(&mut client_connected, || write_socket_frame(stream, &line))?;
                frame_bytes = next_frame_bytes;
                frames.push(line);
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
                if agent_run_cancelled(&session_dir, request.cancellation_id) {
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
    let stderr = stderr_reader
        .join()
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    if !status.success() {
        let diagnostic = safe_agent_process_diagnostic(&stderr);
        if !diagnostic.is_empty() {
            frames.extend(agent_terminal_error_frames(
                request.run_id,
                "EIO",
                &diagnostic,
            ));
        }
    }
    if cancelled {
        return Ok(AgentRunOutcome {
            frames,
            process: AgentProcessOutcome::Cancelled,
        });
    }
    if !status.success() && frames.is_empty() {
        return Ok(AgentRunOutcome {
            frames,
            process: AgentProcessOutcome::Error,
        });
    }
    if let Some(tool_call_frame) = yielded_tool_call {
        if !status.success() {
            return Err(SocketRuntimeError::InvalidAgentOutput);
        }
        write_while_connected(&mut client_connected, || {
            write_socket_frame(stream, &tool_call_frame)
        })?;
        frames.push(tool_call_frame);
        return Ok(AgentRunOutcome {
            frames,
            process: AgentProcessOutcome::Success,
        });
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

fn agent_error_outcome(mut frames: Vec<String>, terminal: Vec<String>) -> AgentRunOutcome {
    if let Some(error) = promoted_recoverable_error(&frames) {
        frames.push(error);
        if let Some(done) = terminal
            .iter()
            .find(|frame| event_type(frame).as_deref() == Some("done"))
        {
            frames.push(done.clone());
        } else {
            frames.extend(terminal);
        }
    } else {
        frames.extend(terminal);
    }
    AgentRunOutcome {
        frames,
        process: AgentProcessOutcome::Error,
    }
}

fn promoted_recoverable_error(frames: &[String]) -> Option<String> {
    frames.iter().rev().find_map(|frame| {
        let value = serde_json::from_str::<Value>(frame).ok()?;
        if value.get("type").and_then(Value::as_str) != Some("error")
            || value.get("recoverable").and_then(Value::as_bool) != Some(true)
        {
            return None;
        }
        Some(
            serde_json::json!({
                "type": "error",
                "run": value.get("run").and_then(Value::as_str)?,
                "code": value.get("code").and_then(Value::as_str)?,
                "message": value.get("message").and_then(Value::as_str)?
            })
            .to_string(),
        )
    })
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

const MAX_AGENT_PROCESS_DIAGNOSTIC_CHARS: usize = 512;
const AGENT_SECRET_MARKERS: [&str; 10] = [
    "sk-",
    "Bearer ",
    "api_key=",
    "apikey=",
    "token=",
    "secret=",
    "password=",
    "authorization=",
    "\"api_key\":\"",
    "\"token\":\"",
];

fn terminal_error_message(run_id: &str, frames: &[String]) -> Option<String> {
    frames.iter().rev().find_map(|frame| {
        let value = serde_json::from_str::<Value>(frame).ok()?;
        if value.get("type").and_then(Value::as_str) != Some("error")
            || value.get("run").and_then(Value::as_str) != Some(run_id)
            || value.get("recoverable").and_then(Value::as_bool) == Some(true)
        {
            return None;
        }
        value
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
    })
}

fn safe_agent_process_diagnostic(stderr: &str) -> String {
    let line = stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or_default();
    let mut diagnostic = line
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>();
    for marker in AGENT_SECRET_MARKERS {
        diagnostic = redact_diagnostic_marker(&diagnostic, marker);
    }
    diagnostic
        .chars()
        .take(MAX_AGENT_PROCESS_DIAGNOSTIC_CHARS)
        .collect()
}

fn redact_diagnostic_marker(value: &str, marker: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find(marker) {
        let value_start = start.saturating_add(marker.len());
        let Some(prefix) = remaining.get(..value_start) else {
            break;
        };
        let Some(tail) = remaining.get(value_start..) else {
            break;
        };
        output.push_str(prefix);
        let value_len = tail
            .find(|character: char| {
                character.is_whitespace() || matches!(character, '"' | '\'' | ',' | '}' | '&')
            })
            .unwrap_or(tail.len());
        if value_len == 0 {
            remaining = tail;
        } else {
            output.push_str("<redacted>");
            let Some(next) = tail.get(value_len..) else {
                break;
            };
            remaining = next;
        }
    }
    output.push_str(remaining);
    output
}

pub(crate) fn agent_process_failed_frames(run_id: &str, stderr: &str) -> Vec<String> {
    let diagnostic = safe_agent_process_diagnostic(stderr);
    let message = if diagnostic.is_empty() {
        "agent process failed".to_owned()
    } else {
        format!("agent process failed: {diagnostic}")
    };
    agent_terminal_error_frames(run_id, "EIO", &message)
}

fn agent_provider_network_denied_frames(run_id: &str) -> Vec<String> {
    agent_terminal_error_frames(
        run_id,
        "EACCES",
        "provider network access denied by agent policy; grant network:default connect",
    )
}

fn agent_invalid_output_frames(run_id: &str) -> Vec<String> {
    agent_terminal_error_frames(run_id, "EPROTO", "agent emitted an invalid event sequence")
}

fn agent_terminal_error_frames(run_id: &str, code: &str, message: &str) -> Vec<String> {
    vec![
        serde_json::json!({
            "type": "error",
            "run": run_id,
            "code": code,
            "message": message
        })
        .to_string(),
        serde_json::json!({"type": "done", "run": run_id, "status": "error"}).to_string(),
    ]
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
    pub(crate) request_id: &'a str,
    pub(crate) run_id: &'a str,
    pub(crate) cancellation_id: &'a str,
    pub(crate) session: &'a str,
    pub(crate) cwd: Option<&'a str>,
    pub(crate) input: &'a str,
    pub(crate) event: Option<&'a Value>,
    pub(crate) origin: Option<&'a InteractionOrigin>,
    pub(crate) channel: Option<&'a runtime::channelenv::ChannelRuntimeContext>,
    pub(crate) history_messages: &'a str,
    pub(crate) tool_context: &'a str,
    pub(crate) debug: Option<SocketDebugTiming>,
}

pub(crate) fn agent_tool_context_for_request(
    cwd: Option<&str>,
) -> Result<String, SocketRuntimeError> {
    let mut context = default_agent_tool_context();
    context.push_str("\n\nCurrent request context:\n");
    context.push_str("- Sandbox cwd: ");
    context.push_str(&prompt_quoted(cwd.unwrap_or("/workspace")));
    context.push('\n');
    context.push_str("- Host workspace configuration: determined by agent policy\n");
    if context.len() > object::executor::MAX_AGENT_TOOL_CONTEXT_BYTES {
        Err(SocketRuntimeError::CannotRunAgent)
    } else {
        Ok(context)
    }
}

pub(crate) fn prompt_quoted(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_error| "\"\"".to_owned())
}

#[cfg(test)]
mod completion_tests {
    use super::*;
    use crate::reference::bootstrap::ensure_runtime_models_from;
    use cortexfs_runtime_client::interaction::{
        InteractionFrame, InteractionRequest, InteractionResult,
    };
    use std::ffi::OsString;
    use std::io;
    use std::net::{Shutdown, TcpListener};
    use std::os::unix::fs::PermissionsExt;

    fn approval_call() -> object::executor::AgentToolCall {
        object::executor::AgentToolCall {
            id: "call-1".to_owned(),
            name: "example.echo".to_owned(),
            args: vec![OsString::from("one")],
        }
    }

    #[test]
    fn provider_network_denial_names_the_required_policy_permission() {
        let frames = agent_provider_network_denied_frames("run-1");
        assert!(frames.first().is_some_and(|frame| frame.contains("EACCES")));
        assert!(
            frames
                .first()
                .is_some_and(|frame| frame.contains("network:default connect"))
        );
    }

    #[test]
    fn approval_response_accepts_only_emitted_run_and_exact_call()
    -> Result<(), Box<dyn std::error::Error>> {
        for response in [
            "",
            "not-json\n",
            "{\"op\":\"approve\",\"run\":\"client-1\",\"id\":\"call-1\",\"decision\":\"allow_once\"}\n",
            "{\"op\":\"approve\",\"run\":\"wrong\",\"id\":\"call-1\",\"decision\":\"allow_once\"}\n",
            "{\"op\":\"approve\",\"run\":\"run-1\",\"id\":\"wrong\",\"decision\":\"allow_once\"}\n",
            "{\"op\":\"approve\",\"run\":\"run-1\",\"id\":\"call-1\",\"decision\":\"deny\"}\n",
        ] {
            let (mut client, mut server) = UnixStream::pair()?;
            client.write_all(response.as_bytes())?;
            client.shutdown(Shutdown::Write)?;
            let approval =
                request_tool_approval(&mut server, "request-1", "run-1", &approval_call())
                    .map_err(|error| io::Error::other(format!("{error:?}")))?;
            assert!(!approval.allowed, "{response:?}");
        }
        let (_client, mut server) = UnixStream::pair()?;
        server.set_read_timeout(Some(Duration::from_millis(10)))?;
        let approval = request_tool_approval(&mut server, "request-1", "run-1", &approval_call())
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        assert!(!approval.allowed);

        let (mut client, mut server) = UnixStream::pair()?;
        client.write_all(
            b"{\"op\":\"approve\",\"run\":\"run-1\",\"id\":\"call-1\",\"decision\":\"allow_once\"}\n",
        )?;
        client.shutdown(Shutdown::Write)?;
        let approval = request_tool_approval(&mut server, "request-1", "run-1", &approval_call())
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        assert!(approval.allowed);

        let (mut client, mut server) = UnixStream::pair()?;
        let response = InteractionFrame::request(InteractionRequest::CommandResult {
            request_id: "other-request".to_owned(),
            session: "default".to_owned(),
            command_id: "call-1".to_owned(),
            result: InteractionResult::Accepted,
        })
        .encode()
        .map_err(io::Error::other)?;
        client.write_all(&response)?;
        client.shutdown(Shutdown::Write)?;
        let approval = request_tool_approval(&mut server, "request-1", "run-1", &approval_call())
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        assert!(!approval.allowed);

        let (mut client, mut server) = UnixStream::pair()?;
        let response = InteractionFrame::request(InteractionRequest::CommandResult {
            request_id: "request-1".to_owned(),
            session: "default".to_owned(),
            command_id: "call-1".to_owned(),
            result: InteractionResult::Accepted,
        })
        .encode()
        .map_err(io::Error::other)?;
        client.write_all(&response)?;
        client.shutdown(Shutdown::Write)?;
        let approval = request_tool_approval(&mut server, "request-1", "run-1", &approval_call())
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        assert!(approval.allowed);
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

        let first = request_tool_approval(&mut server, "request-1", "run-1", &approval_call())
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        assert!(first.allowed);

        let mut second_call = approval_call();
        second_call.id = "call-2".to_owned();
        let replayed = request_tool_approval(&mut server, "request-1", "run-1", &second_call)
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        assert!(!replayed.allowed);
        Ok(())
    }

    #[test]
    fn disconnected_response_disables_later_delivery() {
        let mut connected = true;
        assert_eq!(
            write_while_connected(&mut connected, || {
                Err(SocketRuntimeError::CannotWriteResponse)
            }),
            Ok(())
        );
        assert!(!connected);

        let mut attempted = false;
        assert_eq!(
            write_while_connected(&mut connected, || {
                attempted = true;
                Ok(())
            }),
            Ok(())
        );
        assert!(!attempted);
    }

    #[test]
    fn closed_client_read_half_rejects_socket_response() -> Result<(), Box<dyn std::error::Error>> {
        let (client, mut server) = UnixStream::pair()?;
        client.shutdown(Shutdown::Read)?;
        let response = SocketRuntimeResponse::new(vec![
            serde_json::json!({"type":"start", "run":"run-1"}).to_string(),
        ]);
        assert_eq!(
            write_socket_runtime_response(&mut server, &response),
            Err(SocketRuntimeError::CannotWriteResponse)
        );
        Ok(())
    }

    fn completion_root(name: &str) -> io::Result<tempfile::TempDir> {
        tempfile::Builder::new()
            .prefix(&format!("cfs-{name}-"))
            .tempdir()
    }

    fn test_object_runner() -> Option<PathBuf> {
        let executable = env::current_exe().ok()?;
        let debug = executable.parent()?.parent()?;
        let runner = debug.join("cortexfs-object-runner");
        runner.is_file().then_some(runner)
    }

    fn spawn_provider_upstream()
    -> io::Result<(std::net::SocketAddr, thread::JoinHandle<io::Result<String>>)> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let thread = thread::spawn(move || -> io::Result<String> {
            let (mut stream, _) = listener.accept()?;
            stream.set_read_timeout(Some(Duration::from_secs(5)))?;
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            loop {
                let read = stream.read(&mut chunk)?;
                if read == 0 {
                    break;
                }
                let bytes = chunk
                    .get(..read)
                    .ok_or_else(|| io::Error::other("invalid upstream read"))?;
                request.extend_from_slice(bytes);
                let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
                else {
                    continue;
                };
                let header_bytes = request
                    .get(..header_end)
                    .ok_or_else(|| io::Error::other("invalid upstream headers"))?;
                let headers = String::from_utf8_lossy(header_bytes);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|value| value.trim().parse::<usize>().ok())
                    })
                    .unwrap_or_default();
                if request.len() >= header_end + 4 + length {
                    break;
                }
            }
            let body = concat!(
                "data: {\"choices\":[{\"delta\":{\"content\":\"brokered ok\"},\"finish_reason\":null}]}\n\n",
                "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":3,\"completion_tokens\":2}}\n\n",
                "data: [DONE]\n\n"
            );
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )?;
            String::from_utf8(request)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
        });
        Ok((address, thread))
    }

    fn prepare_provider_fixture(
        root: &Path,
        runner: &Path,
        address: std::net::SocketAddr,
    ) -> io::Result<()> {
        ensure_reference_tree(root).map_err(|error| io::Error::other(format!("{error:?}")))?;
        let providers = root.join("providers.d");
        let cache = root.join("provider-models");
        fs::create_dir_all(&providers)?;
        fs::create_dir_all(&cache)?;
        fs::write(
            providers.join("fixture.json"),
            format!(
                "{{\"name\":\"fixture\",\"base_url\":\"http://{address}/custom\",\"models\":[\"chat\"],\"model_limits\":{{\"chat\":8192}},\"formats\":[\"openai.chat\"]}}\n"
            ),
        )?;
        ensure_runtime_models_from(root, &providers, &cache)
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        fs::copy(runner, root.join("bin/cortexfs-object-runner"))?;
        fs::set_permissions(
            root.join("bin/cortexfs-object-runner"),
            fs::Permissions::from_mode(0o755),
        )?;
        fs::write(root.join("agent/coder.d/model"), "fixture/chat\n")?;
        fs::write(
            root.join("agent/coder.d/policy"),
            "allow coder_t model:fixture/chat use\n",
        )
    }

    fn prepare_empty_reference_fixture(root: &Path) -> io::Result<()> {
        ensure_reference_tree(root).map_err(|error| io::Error::other(format!("{error:?}")))?;
        let providers = root.join("providers.d");
        let cache = root.join("provider-models");
        fs::create_dir_all(&providers)?;
        fs::create_dir_all(&cache)?;
        ensure_runtime_models_from(root, &providers, &cache)
            .map_err(|error| io::Error::other(format!("{error:?}")))
    }

    #[test]
    fn direct_agent_output_rejects_aggregate_overflow() -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let executable = root.path().join("agent");
        fs::write(
            &executable,
            "#!/bin/sh\npayload=$(/usr/bin/head -c 131072 /dev/zero | /usr/bin/tr '\\0' x)\ni=0\nwhile [ \"$i\" -le 8 ]; do printf '{\"type\":\"delta\",\"run\":\"%s\",\"text\":\"%s\"}\\n' \"$CTX_RUN_ID\" \"$payload\"; i=$((i + 1)); done\n",
        )?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
        let identity = AgentUnixIdentity::new(
            nix::unistd::geteuid().as_raw(),
            nix::unistd::getegid().as_raw(),
            [],
        );
        let runtime = AgentExecutableSocketRuntime {
            ctx_root: root.path(),
            source_root: root.path(),
            identity: &identity,
            env: &[],
            session_root: root.path(),
            default_cwd: "/",
            model: None,
            network_allowed: false,
            agent_name: "coder",
            agent_executable: &executable,
            environment: RunEnvironment::Native,
        };
        let request = AgentExecutableRunRequest {
            request_id: "request-1",
            run_id: "run1",
            cancellation_id: "run1",
            session: "default",
            cwd: None,
            input: "overflow",
            event: None,
            origin: None,
            channel: None,
            history_messages: "",
            tool_context: "",
            debug: None,
        };
        let (mut client, mut server) = UnixStream::pair()?;
        let reader = thread::spawn(move || {
            let _ignored = client.read_to_end(&mut Vec::new());
        });
        let outcome = run_agent_request(&mut server, runtime, request, None)
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        drop(server);
        reader
            .join()
            .map_err(|_error| io::Error::other("socket reader panicked"))?;
        assert_eq!(outcome.process, AgentProcessOutcome::Error);
        assert!(outcome.frames.iter().any(|frame| frame.contains("EPROTO")));
        Ok(())
    }

    #[test]
    fn network_allowed_bwrap_agent_streams_through_provider_egress()
    -> Result<(), Box<dyn std::error::Error>> {
        let runner = test_object_runner().ok_or("missing built cortexfs-object-runner")?;
        if !Command::new("/usr/bin/bwrap")
            .args(["--ro-bind", "/", "/", "--unshare-net", "/bin/true"])
            .status()?
            .success()
        {
            return Ok(());
        }

        let root = tempfile::tempdir()?;
        let (address, upstream_thread) = spawn_provider_upstream()?;
        prepare_provider_fixture(root.path(), &runner, address)?;

        let view = derive_agent_runtime_view(root.path(), "coder")
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        let session_root = view.home().join("session");
        fs::create_dir_all(session_root.join("default"))?;
        let control_dir = root.path().join("control");
        fs::create_dir_all(&control_dir)?;
        fs::set_permissions(&control_dir, fs::Permissions::from_mode(0o711))?;
        let mount_table = MountTable::parse("/ctx\t/ctx\tro\trbind,nosuid,nodev\n")
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        let identity = AgentUnixIdentity::new(
            nix::unistd::geteuid().as_raw(),
            nix::unistd::getegid().as_raw(),
            [],
        );
        let env = vec![
            (
                "CTX_PROVIDER_SECRET_PROVIDER".to_owned(),
                "fixture".to_owned(),
            ),
            ("CTX_PROVIDER_SECRET_SLOT".to_owned(), "default".to_owned()),
            (
                "CTX_PROVIDER_SECRET_VALUE".to_owned(),
                "test-secret".to_owned(),
            ),
        ];
        let agent_executable = root.path().join("agent/coder");
        let runtime = AgentExecutableSocketRuntime {
            ctx_root: root.path(),
            source_root: root.path(),
            identity: &identity,
            env: &env,
            session_root: &session_root,
            default_cwd: "/workspace",
            model: Some("fixture/chat"),
            network_allowed: true,
            agent_name: "coder",
            agent_executable: &agent_executable,
            environment: RunEnvironment::Sandbox {
                program: Path::new("/usr/bin/bwrap"),
                mount_table: &mount_table,
                control_dir: Some(&control_dir),
            },
        };
        let request = AgentExecutableRunRequest {
            request_id: "request-1",
            run_id: "run1",
            cancellation_id: "run1",
            session: "default",
            cwd: None,
            input: "say hello",
            event: None,
            origin: None,
            channel: None,
            history_messages: "",
            tool_context: "",
            debug: None,
        };
        let (mut client, mut server) = UnixStream::pair()?;
        let outcome = run_agent_request(&mut server, runtime, request, None)
            .map_err(|error| io::Error::other(format!("{error:?}")))?;
        server.shutdown(Shutdown::Write)?;
        let mut delivered = String::new();
        client.read_to_string(&mut delivered)?;
        let frames = outcome.frames.join("\n");
        assert!(frames.contains("brokered ok"), "{frames}");
        assert!(frames.contains("\"type\":\"usage\""), "{frames}");
        assert!(frames.contains("\"type\":\"done\""), "{frames}");
        assert!(delivered.contains("brokered ok"), "{delivered}");
        let captured = upstream_thread
            .join()
            .map_err(|_panic| io::Error::other("upstream panicked"))??;
        assert!(
            captured.starts_with("POST /custom/v1/chat/completions HTTP/1.1\r\n"),
            "{captured}"
        );
        assert!(
            captured.contains(&format!("Host: {address}\r\n")),
            "{captured}"
        );
        assert!(
            captured
                .to_ascii_lowercase()
                .contains("authorization: bearer test-secret\r\n"),
            "{captured}"
        );
        Ok(())
    }

    #[test]
    fn owned_child_channel_uses_canonical_parent_home() -> io::Result<()> {
        let root = completion_root("canonical-child-channel")?;
        assert!(prepare_empty_reference_fixture(root.path()).is_ok());
        let parent = derive_agent_runtime_view(root.path(), "coder");
        assert!(parent.is_ok());
        let Ok(parent) = parent else {
            return Ok(());
        };
        let channel = canonical_owned_child_channel(
            root.path(),
            parent.owner(),
            "coder",
            "default",
            "worker-1",
        );
        assert_eq!(
            channel,
            Ok(parent.home().join("session/default/context/child/worker-1"))
        );
        assert_ne!(
            channel.unwrap_or_default(),
            root.path()
                .join("agent/coder.d/session/default/context/child/worker-1")
        );
        Ok(())
    }

    #[test]
    fn owned_child_channel_rejects_parent_owner_mismatch() -> io::Result<()> {
        let root = completion_root("child-channel-owner-mismatch")?;
        assert!(prepare_empty_reference_fixture(root.path()).is_ok());
        let parent = derive_agent_runtime_view(root.path(), "coder");
        assert!(parent.is_ok());
        let Ok(parent) = parent else {
            return Ok(());
        };
        assert_eq!(
            canonical_owned_child_channel(
                root.path(),
                parent.owner().saturating_add(1),
                "coder",
                "default",
                "worker-1",
            ),
            Err(SocketRuntimeError::CannotRunAgent)
        );
        Ok(())
    }

    #[test]
    fn parent_reference_classifier_is_exact() {
        assert_eq!(
            classify_parent_reference("agent:architect"),
            ParentReference::Static { agent: "architect" }
        );
        assert_eq!(
            classify_parent_reference("agent:parent session:session run:run"),
            ParentReference::Delegated {
                agent: "parent",
                session: "session",
                run: "run"
            }
        );
        for malformed in [
            "agent:parent session:session",
            "agent:parent run:run",
            "agent:parent session:session run:run extra:x",
            "agent:parent agent:other session:session run:run",
            "session:session agent:parent run:run",
            "agent:bad/name",
            "agent:parent session:bad/name run:run",
            "agent:parent session:session run:bad/name",
            "agent:parent  session:session run:run",
        ] {
            assert_eq!(
                classify_parent_reference(malformed),
                ParentReference::Malformed
            );
        }
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

    #[test]
    fn process_diagnostic_is_bounded_and_redacts_credentials() {
        let diagnostic = safe_agent_process_diagnostic(
            "cortexfs-object-runner: request failed api_key=sk-live-secret Bearer abc\nsecond",
        );
        assert!(diagnostic.contains("api_key=<redacted>"));
        assert!(diagnostic.contains("Bearer <redacted>"));
        assert!(!diagnostic.contains("live-secret"));
        assert!(!diagnostic.contains("abc"));
        assert!(diagnostic.chars().count() <= MAX_AGENT_PROCESS_DIAGNOSTIC_CHARS);
    }

    #[test]
    fn continuation_context_marks_completed_tool_call() {
        let call = object::executor::AgentToolCall {
            id: "call-1".to_owned(),
            name: "tsh".to_owned(),
            args: vec![OsString::from("shell.exec"), OsString::from("pwd")],
        };
        let context = continuation_tool_context("base", &call, "ok");
        assert!(context.contains("Latest host tool completion:"));
        assert!(context.contains(r#"["shell.exec","pwd"]"#));
        assert!(context.contains("do not repeat this call"));
    }

    #[test]
    fn socket_child_helper_forwards_some_and_none_windows_exactly() {
        CAPTURE_SOCKET_CHILD_WINDOW.with(|capture| capture.borrow_mut().clear());
        CAPTURE_SOCKET_CHILD_WINDOW_ENABLED.with(|enabled| enabled.set(true));
        for (child, window) in [("known", Some(2048)), ("auto", None)] {
            let result = create_socket_child(
                Path::new("/source"),
                Path::new("/ctx"),
                runtime::control::CreateChildRequest {
                    agent: "parent".to_owned(),
                    session: "session".to_owned(),
                    run: "run".to_owned(),
                    child: child.to_owned(),
                    child_session: format!("{child}-session"),
                    path: None,
                    window,
                    input: "work".to_owned(),
                    life: "owned".to_owned(),
                },
            );
            assert_eq!(
                result,
                Err(runtime::control::RunCapabilityError::CannotCreate)
            );
        }
        CAPTURE_SOCKET_CHILD_WINDOW_ENABLED.with(|enabled| enabled.set(false));
        assert_eq!(
            CAPTURE_SOCKET_CHILD_WINDOW.with(|capture| capture.borrow().clone()),
            [Some(2048), None]
        );
    }

    /// 校验 `update_socket_prompt` 只原子替换合法 prompt 控制文件：
    /// 非 prompt 控制、NUL 内容与缺失 agent 目录都 fail closed 且不落盘。
    #[test]
    fn socket_prompt_update_replaces_only_prompt_controls() -> Result<(), Box<dyn std::error::Error>>
    {
        let root = tempfile::tempdir()?;
        let control_dir = root.path().join("agent/self.d");
        fs::create_dir_all(&control_dir)?;
        fs::write(control_dir.join("system.md"), "old prompt\n")?;
        let request = |control: &str, content: &str| runtime::control::UpdatePromptRequest {
            agent: "self".to_owned(),
            session: "session-1".to_owned(),
            run: "run-1".to_owned(),
            control: control.to_owned(),
            content: content.to_owned(),
        };
        assert_eq!(
            update_socket_prompt(root.path(), &request("window", "auto\n")),
            Err(runtime::control::RunCapabilityError::InvalidFrame)
        );
        assert_eq!(
            update_socket_prompt(root.path(), &request("system.md", "bad\0content")),
            Err(runtime::control::RunCapabilityError::InvalidFrame)
        );
        assert_eq!(
            fs::read_to_string(control_dir.join("system.md"))?,
            "old prompt\n"
        );
        let mut missing = request("system.md", "new prompt\n");
        missing.agent = "absent".to_owned();
        assert_eq!(
            update_socket_prompt(root.path(), &missing),
            Err(runtime::control::RunCapabilityError::CannotWrite)
        );
        update_socket_prompt(
            root.path(),
            &request("system.md", "You iterate yourself.\n"),
        )
        .map_err(|error| io::Error::other(format!("cannot update prompt: {error:?}")))?;
        assert_eq!(
            fs::read_to_string(control_dir.join("system.md"))?,
            "You iterate yourself.\n"
        );
        update_socket_prompt(root.path(), &request("prompt.template.md", "{{input}}\n"))
            .map_err(|error| io::Error::other(format!("cannot create template: {error:?}")))?;
        assert_eq!(
            fs::read_to_string(control_dir.join("prompt.template.md"))?,
            "{{input}}\n"
        );
        Ok(())
    }
}
