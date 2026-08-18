use super::*;

#[cfg(test)]
use crate::support::command::ID;
use crate::support::command::SETPRIV;
use crate::support::plain::{open_plain_directory, read_small_text_file};

pub(crate) fn command_for_agent_identity(
    program: impl AsRef<std::ffi::OsStr>,
    identity: &AgentUnixIdentity,
) -> Command {
    if !nix::unistd::geteuid().is_root() {
        return Command::new(program);
    }
    let mut command = Command::new(SETPRIV);
    command.args(["--reuid", &identity.uid().to_string()]);
    command.args(["--regid", &identity.gid().to_string()]);
    if identity.groups().is_empty() {
        command.arg("--clear-groups");
    } else {
        command.arg("--groups").arg(
            identity
                .groups()
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    command.arg("--").arg(program);
    command
}

#[cfg(test)]
#[expect(
    clippy::items_after_test_module,
    reason = "identity tests stay beside the command constructor"
)]
mod identity_tests {
    use super::*;

    #[test]
    fn root_caller_applies_uid_gid_and_supplementary_groups()
    -> Result<(), Box<dyn std::error::Error>> {
        if !nix::unistd::geteuid().is_root() {
            return Ok(());
        }
        let identity = AgentUnixIdentity::new(65_534, 65_534, [1]);
        let mut command = command_for_agent_identity(ID, &identity);
        command.arg("-u");
        let uid = command.output()?;
        assert!(uid.status.success());
        assert_eq!(String::from_utf8(uid.stdout)?.trim(), "65534");

        let mut command = command_for_agent_identity(ID, &identity);
        command.arg("-g");
        let gid = command.output()?;
        assert!(gid.status.success());
        assert_eq!(String::from_utf8(gid.stdout)?.trim(), "65534");

        let mut command = command_for_agent_identity(ID, &identity);
        command.arg("-G");
        let groups = command.output()?;
        assert!(groups.status.success());
        let groups = String::from_utf8(groups.stdout)?;
        let groups = groups.split_whitespace().collect::<Vec<_>>();
        assert!(groups.contains(&"65534"));
        assert!(groups.contains(&"1"));
        Ok(())
    }

    #[test]
    fn non_root_caller_keeps_existing_identity() -> Result<(), Box<dyn std::error::Error>> {
        if nix::unistd::geteuid().is_root() {
            return Ok(());
        }
        let mut command =
            command_for_agent_identity(ID, &AgentUnixIdentity::new(65_534, 65_534, [1]));
        command.arg("-u");
        let output = command.output()?;
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout)?.trim(),
            nix::unistd::geteuid().as_raw().to_string()
        );
        Ok(())
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
    let Ok(events) = columnar::read_text(
        session_dir,
        columnar::Stream::Events,
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

enum AgentTerminal<'a> {
    Success {
        assistant: Option<String>,
        done: &'a str,
    },
    Error {
        error: &'a str,
        done: &'a str,
    },
}

pub(crate) struct AgentFrameBatch<'a> {
    terminal: Option<AgentTerminal<'a>>,
}

impl<'a> AgentFrameBatch<'a> {
    pub(crate) fn parse(run_id: &str, frames: &'a [String]) -> Self {
        let mut assistant = String::new();
        let mut error = None;
        let mut done = None;

        for frame in frames {
            let Ok(value) = serde_json::from_str::<Value>(frame) else {
                continue;
            };
            let event = value.get("type").and_then(Value::as_str);
            if value.get("run").and_then(Value::as_str) != Some(run_id) {
                continue;
            }
            match event {
                Some("error")
                    if value.get("recoverable").and_then(Value::as_bool) != Some(true) =>
                {
                    error = Some(frame.as_str());
                }
                Some("done") => match value.get("status").and_then(Value::as_str) {
                    Some("ok") => done = Some((frame.as_str(), true)),
                    Some("error") => done = Some((frame.as_str(), false)),
                    _ => {}
                },
                _ => push_assistant_text(&mut assistant, &value),
            }
        }
        let terminal = done.and_then(|(done, success)| {
            if success {
                Some(AgentTerminal::Success {
                    assistant: (!assistant.is_empty()).then_some(assistant),
                    done,
                })
            } else {
                error.map(|error| AgentTerminal::Error { error, done })
            }
        });
        Self { terminal }
    }

    pub(crate) fn settle(
        self,
        session_dir: &Path,
        run_id: &str,
    ) -> Result<bool, SocketSessionRecordError> {
        let Some(terminal) = self.terminal else {
            return Ok(false);
        };
        require_socket_session_files(session_dir)?;
        let history = columnar::HistoryGuard::exclusive(session_dir)
            .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
        if !active_session_run_matches_locked(&history, session_dir, run_id)? {
            return Ok(false);
        }
        history
            .refresh_claims()
            .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
        let (state, error_code) = match terminal {
            AgentTerminal::Success {
                assistant: Some(assistant),
                done,
            } => {
                record_assistant_response_locked(&history, session_dir, run_id, &assistant, done)?;
                ("done", None)
            }
            AgentTerminal::Success {
                assistant: None,
                done,
            } => {
                history
                    .append(columnar::Stream::Events, &[done])
                    .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
                ("done", None)
            }
            AgentTerminal::Error { error, done } => {
                history
                    .append(columnar::Stream::Events, &[error, done])
                    .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
                ("error", stable_error_code(error))
            }
        };
        history
            .refresh_claims()
            .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
        transition_active_session_run_locked(
            &history,
            session_dir,
            run_id,
            state,
            error_code.as_deref(),
        )
    }
}

fn stable_error_code(frame: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(frame).ok()?;
    let code = value.get("code")?.as_str()?;
    ((1..=32).contains(&code.len())
        && code
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_'))
    .then(|| code.to_owned())
}

pub(crate) fn record_tool_approval_frames(
    session_dir: &Path,
    frames: &[String; 2],
) -> Result<(), SocketSessionRecordError> {
    require_socket_session_files(session_dir)?;
    append_session_lines(
        session_dir,
        "events.jsonl",
        &[frames[0].as_str(), frames[1].as_str()],
    )
}

fn push_assistant_text(output: &mut String, value: &Value) {
    let event = value.get("type").and_then(Value::as_str);
    if matches!(event, Some("delta" | "reasoning_delta"))
        && let Some(text) = value.get("text").and_then(Value::as_str)
    {
        output.push_str(text);
        return;
    }
    if matches!(event, Some("message" | "reasoning_message"))
        && value.get("role").and_then(Value::as_str) == Some("assistant")
        && let Some(text) = message_event_text(value)
    {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&text);
    }
}

pub(crate) fn message_event_text(value: &Value) -> Option<String> {
    let mut text = String::new();
    for part in value.get("content")?.as_array()? {
        if part.get("type").and_then(Value::as_str) == Some("text")
            && let Some(value) = part.get("text").and_then(Value::as_str)
        {
            text.push_str(value);
        }
    }
    (!text.is_empty()).then_some(text)
}
