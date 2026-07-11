use super::*;

use crate::support::plain::{open_plain_directory, read_small_text_file};

pub(crate) fn apply_agent_identity_to_command(command: &mut Command, identity: &AgentUnixIdentity) {
    if nix::unistd::geteuid().is_root() {
        command.gid(identity.gid()).uid(identity.uid());
    }
}

pub(crate) fn open_agent_executable_no_follow(path: &Path) -> Result<fs::File, SocketRuntimeError> {
    if !path.is_absolute() {
        return Err(SocketRuntimeError::InvalidAgentExecutable);
    }
    let parent = path
        .parent()
        .ok_or(SocketRuntimeError::InvalidAgentExecutable)?;
    let parent_dir = open_plain_directory(parent)
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

pub(crate) fn terminate_agent_process_group(child: &mut Child) {
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

pub(crate) fn signal_agent_process_group(pid: i32, signal: nix::sys::signal::Signal) {
    let _ignored = nix::sys::signal::kill(nix::unistd::Pid::from_raw(-pid), signal);
}

pub(crate) fn event_type(line: &str) -> Option<String> {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_owned))
}

pub(crate) fn agent_run_cancelled(session_dir: &Path, run_id: &str) -> bool {
    let Ok(state) = read_small_text_file(
        &session_dir.join("state"),
        MAX_SOCKET_RUNTIME_SMALL_FILE_BYTES,
    ) else {
        return false;
    };
    if state.trim() != "cancelled" {
        return false;
    }
    let Ok(events) = read_small_text_file(
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

pub(crate) fn assistant_text_from_event_frames(frames: &[String]) -> Option<String> {
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

pub(crate) fn record_agent_error_from_event_frames(
    session_dir: &Path,
    run_id: &str,
    frames: &[String],
) -> Result<bool, SocketSessionRecordError> {
    let mut error = None;
    let mut terminal = None;
    for frame in frames {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            continue;
        };
        if value.get("run").and_then(Value::as_str) != Some(run_id) {
            continue;
        }
        match value.get("type").and_then(Value::as_str) {
            Some("error") => error = Some(frame.as_str()),
            Some("done") => {
                terminal = None;
                if value.get("status").and_then(Value::as_str) == Some("error")
                    && let Some(error) = error
                {
                    terminal = Some([error, frame.as_str()]);
                }
            }
            _ => {}
        }
    }
    let Some(terminal) = terminal else {
        return Ok(false);
    };

    require_socket_session_files(session_dir)?;
    let events = read_small_text_file(
        &session_dir.join("events.jsonl"),
        MAX_SOCKET_RUNTIME_EVENTS_BYTES,
    )
    .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    let mut already_done = false;
    for_each_jsonl_line(&events, |_line_number, line| {
        already_done |= serde_json::from_str::<Value>(line).is_ok_and(|value| {
            value.get("type").and_then(Value::as_str) == Some("done")
                && value.get("run").and_then(Value::as_str) == Some(run_id)
        });
    });
    if already_done {
        return Ok(true);
    }

    append_session_lines(session_dir, "events.jsonl", &terminal)?;
    set_session_state(session_dir, "error")?;
    Ok(true)
}

pub(crate) fn record_tool_results_from_event_frames(
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

pub(crate) fn tool_name_for_call<'a>(
    calls: &'a [(String, String)],
    tool_call_id: &str,
) -> Option<&'a String> {
    calls
        .iter()
        .find_map(|call| (call.0 == tool_call_id).then_some(&call.1))
}

pub(crate) fn tool_result_content_text(content: Option<&Value>) -> String {
    if let Some(value) = content.and_then(Value::as_str) {
        return value.to_owned();
    }
    content.map_or_else(String::new, Value::to_string)
}

pub(crate) fn message_event_text(value: &Value) -> Option<String> {
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
