use crate::*;

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct AgentEventRender {
    pub(crate) exit_code: u8,
    pub(crate) interrupted: bool,
}

pub(crate) fn render_agent_events(stream: UnixStream) -> Result<ExitCode, CliError> {
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .map_err(|error| {
            CliError::unavailable(format!("cannot configure socket progress: {error}"))
        })?;
    let reader = io::BufReader::new(stream);
    Ok(ExitCode::from(
        render_agent_event_lines(reader, None)?.exit_code,
    ))
}

pub(crate) fn render_agent_events_interruptible(
    stream: UnixStream,
    interrupt: &AtomicBool,
) -> Result<AgentEventRender, CliError> {
    let reader = io::BufReader::new(stream);
    render_agent_event_lines(reader, Some(interrupt))
}

pub(crate) fn render_agent_event_lines(
    mut reader: impl BufRead,
    interrupt: Option<&AtomicBool>,
) -> Result<AgentEventRender, CliError> {
    let mut saw_delta = false;
    let mut usage_totals = AgentUsageTotals::default();
    let mut exit = ExitCode::SUCCESS;
    let mut quiet_since = std::time::Instant::now();
    let mut next_waiting_notice = Duration::from_secs(3);
    let mut waiting_status_active = false;
    let mut response_bytes: usize = 0;
    let mut events: usize = 0;
    let mut line = String::new();
    let mut pending_frame = Vec::new();
    loop {
        line.clear();
        match read_agent_socket_event_line_limited(&mut reader, &mut pending_frame, &mut line) {
            Ok(None) => break,
            Ok(Some(bytes)) => {
                response_bytes = response_bytes.checked_add(bytes).ok_or_else(|| {
                    CliError::unavailable("agent response exceeds response limit")
                })?;
                if response_bytes > MAX_AGENT_RESPONSE_BYTES {
                    return Err(CliError::unavailable(format!(
                        "agent response exceeds {MAX_AGENT_RESPONSE_BYTES} bytes"
                    )));
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                if interrupt.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
                    clear_waiting_status_if_active(&mut waiting_status_active)?;
                    return Ok(AgentEventRender {
                        exit_code: exit_code_u8(exit),
                        interrupted: true,
                    });
                }
                update_waiting_status(
                    quiet_since.elapsed(),
                    &mut next_waiting_notice,
                    &mut waiting_status_active,
                )?;
                continue;
            }
            Err(error) => {
                clear_waiting_status_if_active(&mut waiting_status_active)?;
                return Err(CliError::unavailable(format!(
                    "cannot read socket response: {error}"
                )));
            }
        }
        clear_waiting_status_if_active(&mut waiting_status_active)?;
        quiet_since = std::time::Instant::now();
        next_waiting_notice = Duration::from_secs(3);
        events += 1;
        if events > MAX_AGENT_EVENTS {
            return Err(CliError::unavailable(format!(
                "agent response exceeds {MAX_AGENT_EVENTS} events"
            )));
        }
        let line = line.trim_end_matches(['\r', '\n']);
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            print_line(line)?;
            continue;
        };
        match value.get("type").and_then(serde_json::Value::as_str) {
            Some("delta" | "reasoning_delta") => {
                if let Some(text) = json_text_field(&value) {
                    print_terminal_text(text)?;
                    saw_delta = true;
                }
            }
            Some("message")
                if value.get("role").and_then(serde_json::Value::as_str) == Some("tool") =>
            {
                write_terminal_diagnostic(&tool_result_diagnostic(&value))?;
            }
            Some("message" | "reasoning_message") if !saw_delta => {
                if let Some(text) = json_text_field(&value) {
                    print_terminal_line(text)?;
                }
            }
            Some("tool_call") => {
                write_terminal_diagnostic(&tool_running_diagnostic(&value))?;
            }
            Some("usage") => {
                if let Some(diagnostic) = usage_totals.record_event(&value) {
                    write_terminal_diagnostic(&diagnostic)?;
                }
            }
            Some("debug") => {
                if let Some(diagnostic) = debug_timing_diagnostic(&value) {
                    write_terminal_diagnostic(&diagnostic)?;
                }
            }
            Some("error") => {
                let code = value
                    .get("code")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("EIO");
                let message = value
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("runtime error");
                write_terminal_diagnostic(&error_diagnostic(code, message))?;
                exit = ExitCode::from(1);
            }
            Some("pong") => print_line("pong")?,
            Some("done") if saw_delta => {
                print_terminal_text("\n")?;
                saw_delta = false;
            }
            _ => {}
        }
    }
    Ok(AgentEventRender {
        exit_code: exit_code_u8(exit),
        interrupted: false,
    })
}

pub(crate) fn clear_waiting_status_if_active(active: &mut bool) -> Result<(), CliError> {
    if *active {
        clear_terminal_status()?;
        *active = false;
    }
    Ok(())
}

pub(crate) fn update_waiting_status(
    elapsed: Duration,
    next_notice: &mut Duration,
    active: &mut bool,
) -> Result<(), CliError> {
    if elapsed < *next_notice {
        return Ok(());
    }
    write_terminal_status(&waiting_diagnostic(elapsed.as_secs()))?;
    *active = true;
    *next_notice += Duration::from_secs(3);
    Ok(())
}

pub(crate) fn read_agent_socket_event_line_limited(
    reader: &mut impl BufRead,
    pending_frame: &mut Vec<u8>,
    line: &mut String,
) -> io::Result<Option<usize>> {
    if pending_frame.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "agent socket response frame exceeds limit",
        ));
    }
    let remaining = MAX_SOCKET_FRAME_BYTES
        .saturating_add(1)
        .saturating_sub(pending_frame.len());
    let limit = u64::try_from(remaining).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("socket frame read limit is invalid: {error}"),
        )
    })?;
    let read = reader.take(limit).read_until(b'\n', pending_frame)?;
    if read == 0 && pending_frame.is_empty() {
        return Ok(None);
    }
    if pending_frame.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "agent socket response frame exceeds limit",
        ));
    }
    let bytes = std::mem::take(pending_frame);
    *line = String::from_utf8(bytes).map_err(|_error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "agent socket response is not UTF-8",
        )
    })?;
    Ok(Some(line.len()))
}

pub(crate) fn exit_code_u8(code: ExitCode) -> u8 {
    u8::from(code != ExitCode::SUCCESS)
}

pub(crate) const MAX_AGENT_RESPONSE_BYTES: usize = MAX_SOCKET_FRAME_BYTES * 4;
pub(crate) const MAX_AGENT_EVENTS: usize = 8192;
#[cfg(test)]
pub(crate) const MAX_BUFFERED_AGENT_RESPONSE_BYTES: usize = MAX_AGENT_RESPONSE_BYTES;
#[cfg(test)]
pub(crate) const MAX_BUFFERED_AGENT_RENDERED_BYTES: usize = MAX_SOCKET_FRAME_BYTES;
#[cfg(test)]
pub(crate) const MAX_BUFFERED_AGENT_EVENTS: usize = MAX_AGENT_EVENTS;
#[cfg(test)]
pub(crate) const MAX_BUFFERED_AGENT_DIAGNOSTICS: usize = 1024;

#[cfg(test)]
#[derive(Debug, Eq, PartialEq)]
pub(crate) struct BufferedAgentEvents {
    pub(crate) output: String,
    pub(crate) diagnostics: Vec<String>,
    pub(crate) exit_code: u8,
    pub(crate) interrupted: bool,
}

#[cfg(test)]
pub(crate) fn collect_agent_events_buffered(
    reader: impl BufRead,
) -> Result<BufferedAgentEvents, CliError> {
    collect_agent_events_buffered_with(reader, None)
}

#[cfg(test)]
pub(crate) fn collect_agent_events_buffered_interruptible(
    reader: impl BufRead,
    interrupt: &AtomicBool,
) -> Result<BufferedAgentEvents, CliError> {
    collect_agent_events_buffered_with(reader, Some(interrupt))
}

#[cfg(test)]
pub(crate) fn collect_agent_events_buffered_with(
    mut reader: impl BufRead,
    interrupt: Option<&AtomicBool>,
) -> Result<BufferedAgentEvents, CliError> {
    let mut saw_delta = false;
    let mut usage_totals = AgentUsageTotals::default();
    let mut output = String::new();
    let mut diagnostics = Vec::new();
    let mut exit_code = 0;
    let mut response_bytes: usize = 0;
    let mut events = 0;
    let mut line = String::new();
    let mut pending_frame = Vec::new();
    loop {
        line.clear();
        match read_agent_socket_event_line_limited(&mut reader, &mut pending_frame, &mut line) {
            Ok(None) => break,
            Ok(Some(bytes)) => {
                response_bytes = response_bytes.checked_add(bytes).ok_or_else(|| {
                    CliError::unavailable("agent response exceeds buffered response limit")
                })?;
                if response_bytes > MAX_BUFFERED_AGENT_RESPONSE_BYTES {
                    return Err(CliError::unavailable(format!(
                        "agent response exceeds {MAX_BUFFERED_AGENT_RESPONSE_BYTES} buffered bytes"
                    )));
                }
            }
            Err(error)
                if interrupt.is_some()
                    && matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
            {
                if interrupt.is_some_and(|flag| flag.load(Ordering::SeqCst)) {
                    return Ok(BufferedAgentEvents {
                        output,
                        diagnostics,
                        exit_code,
                        interrupted: true,
                    });
                }
                continue;
            }
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot read socket response: {error}"
                )));
            }
        }
        events += 1;
        if events > MAX_BUFFERED_AGENT_EVENTS {
            return Err(CliError::unavailable(format!(
                "agent response exceeds {MAX_BUFFERED_AGENT_EVENTS} buffered events"
            )));
        }
        let line = line.trim_end_matches(['\r', '\n']);
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            push_buffered_output(&mut output, line)?;
            push_buffered_output(&mut output, "\n")?;
            continue;
        };
        match value.get("type").and_then(serde_json::Value::as_str) {
            Some("delta" | "reasoning_delta") => {
                if let Some(text) = json_text_field(&value) {
                    push_buffered_output(&mut output, text)?;
                    saw_delta = true;
                }
            }
            Some("message")
                if value.get("role").and_then(serde_json::Value::as_str) == Some("tool") =>
            {
                push_buffered_diagnostic(&mut diagnostics, tool_result_diagnostic(&value))?;
            }
            Some("message" | "reasoning_message") if !saw_delta => {
                if let Some(text) = json_text_field(&value) {
                    push_buffered_output(&mut output, text)?;
                    push_buffered_output(&mut output, "\n")?;
                }
            }
            Some("tool_call") => {
                push_buffered_diagnostic(&mut diagnostics, tool_running_diagnostic(&value))?;
            }
            Some("usage") => {
                if let Some(diagnostic) = usage_totals.record_event(&value) {
                    push_buffered_diagnostic(&mut diagnostics, diagnostic)?;
                }
            }
            Some("error") => {
                let code = value
                    .get("code")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("EIO");
                let message = value
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("runtime error");
                push_buffered_diagnostic(&mut diagnostics, error_diagnostic(code, message))?;
                exit_code = 1;
            }
            Some("pong") => push_buffered_output(&mut output, "pong\n")?,
            Some("done") if saw_delta => {
                push_buffered_output(&mut output, "\n")?;
                saw_delta = false;
            }
            _ => {}
        }
    }
    Ok(BufferedAgentEvents {
        output,
        diagnostics,
        exit_code,
        interrupted: false,
    })
}

#[derive(Default)]
struct AgentUsageTotals {
    input_tokens: u64,
    output_tokens: u64,
}

impl AgentUsageTotals {
    fn record_event(&mut self, value: &serde_json::Value) -> Option<String> {
        let input_delta = value
            .get("input_tokens")
            .and_then(serde_json::Value::as_u64)?;
        let output_delta = value
            .get("output_tokens")
            .and_then(serde_json::Value::as_u64)?;
        self.input_tokens = self.input_tokens.saturating_add(input_delta);
        self.output_tokens = self.output_tokens.saturating_add(output_delta);
        let color = color_enabled();
        Some(format!(
            "{} {} {}",
            styled(color, ANSI_DIM, "tokens"),
            styled(
                color,
                ANSI_CYAN,
                &format!("in +{input_delta}/{}", self.input_tokens)
            ),
            styled(
                color,
                ANSI_GREEN,
                &format!("out +{output_delta}/{}", self.output_tokens)
            )
        ))
    }
}
