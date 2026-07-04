fn waiting_diagnostic(seconds: u64) -> String {
    let color = color_enabled();
    format!(
        "{} {} {}",
        styled(color, ANSI_DIM, "agent"),
        styled(color, ANSI_CYAN, &format!("waiting {seconds}s")),
        styled(color, ANSI_DIM, "for first event...")
    )
}

fn debug_timing_diagnostic(value: &serde_json::Value) -> Option<String> {
    let elapsed = value
        .get("elapsed_ms")
        .and_then(serde_json::Value::as_u64)?;
    let stage = value
        .get("stage")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("event");
    let color = color_enabled();
    Some(format!(
        "{} {} {}",
        styled(color, ANSI_DIM, "[debug timing]"),
        styled(color, ANSI_CYAN, &format!("+{elapsed}ms")),
        styled(color, ANSI_DIM, &terminal_safe_text(stage))
    ))
}

fn tool_running_diagnostic(name: &str) -> String {
    let color = color_enabled();
    let name = terminal_safe_text(name);
    format!(
        "{} {} {}",
        styled(color, ANSI_BOLD_YELLOW, "tool"),
        styled(color, ANSI_CYAN, &name),
        styled(color, ANSI_DIM, "running")
    )
}

fn tool_result_diagnostic(value: &serde_json::Value) -> String {
    let color = color_enabled();
    let name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("tool");
    let name = terminal_safe_text(name);
    let bytes = tool_message_content_bytes(value);
    format!(
        "{} {} {} {}",
        styled(color, ANSI_BOLD_YELLOW, "tool"),
        styled(color, ANSI_CYAN, &name),
        styled(color, ANSI_GREEN, "done"),
        styled(color, ANSI_DIM, &format!("{bytes} bytes"))
    )
}

fn tool_message_content_bytes(value: &serde_json::Value) -> usize {
    match value.get("content") {
        Some(content) if content.is_string() => {
            content.as_str().map_or(0, str::len)
        }
        Some(content) if content.is_array() => content.as_array().map_or(0, |items| {
            items
                .iter()
                .map(|item| {
                    item.get("content")
                        .or_else(|| item.get("text"))
                        .and_then(serde_json::Value::as_str)
                        .map_or_else(|| item.to_string().len(), str::len)
                })
                .sum()
        }),
        Some(other) => other.to_string().len(),
        None => 0,
    }
}

fn error_diagnostic(code: &str, message: &str) -> String {
    let color = color_enabled();
    let code = terminal_safe_text(code);
    let message = terminal_safe_text(message);
    format!(
        "{} {}: {}",
        styled(color, ANSI_RED, "error"),
        styled(color, ANSI_BOLD_YELLOW, &code),
        message
    )
}

#[cfg(test)]
fn push_buffered_output(output: &mut String, text: &str) -> Result<(), CliError> {
    let bytes = output.len().checked_add(text.len()).ok_or_else(|| {
        CliError::unavailable("agent output exceeds buffered output limit")
    })?;
    if bytes > MAX_BUFFERED_AGENT_RENDERED_BYTES {
        return Err(CliError::unavailable(format!(
            "agent output exceeds {MAX_BUFFERED_AGENT_RENDERED_BYTES} buffered bytes"
        )));
    }
    output.push_str(text);
    Ok(())
}

#[cfg(test)]
fn push_buffered_diagnostic(
    diagnostics: &mut Vec<String>,
    diagnostic: String,
) -> Result<(), CliError> {
    if diagnostics.len() >= MAX_BUFFERED_AGENT_DIAGNOSTICS {
        return Err(CliError::unavailable(format!(
            "agent response exceeds {MAX_BUFFERED_AGENT_DIAGNOSTICS} buffered diagnostics"
        )));
    }
    diagnostics.push(diagnostic);
    Ok(())
}

fn json_text_field(value: &serde_json::Value) -> Option<&str> {
    if let Some(text) = value.get("text").and_then(serde_json::Value::as_str) {
        return Some(text);
    }
    let content = value.get("content")?;
    if let Some(text) = content.as_str() {
        return Some(text);
    }
    content.as_array()?.iter().find_map(|item| {
        item.get("text")
            .or_else(|| item.get("content"))
            .and_then(serde_json::Value::as_str)
    })
}

fn print_terminal_text(text: &str) -> Result<(), CliError> {
    let text = terminal_safe_text(text);
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|error| CliError::unavailable(format!("stdout write failed: {error}")))
}

fn print_terminal_line(line: &str) -> Result<(), CliError> {
    let line = terminal_safe_text(line);
    print_line(&line)
}

fn write_terminal_error(line: &str) -> Result<(), CliError> {
    let line = terminal_safe_text(line);
    write_error(&line).map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))
}

fn write_terminal_diagnostic(line: &str) -> Result<(), CliError> {
    write_error(line).map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))
}

fn write_terminal_status(line: &str) -> Result<(), CliError> {
    let line = terminal_safe_text(line);
    let mut stderr = io::stderr().lock();
    stderr
        .write_all(b"\r\x1b[2K")
        .and_then(|()| stderr.write_all(line.as_bytes()))
        .and_then(|()| stderr.flush())
        .map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))
}

fn clear_terminal_status() -> Result<(), CliError> {
    let mut stderr = io::stderr().lock();
    stderr
        .write_all(b"\r\x1b[2K")
        .and_then(|()| stderr.flush())
        .map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))
}

fn terminal_safe_text(text: &str) -> String {
    let mut safe = String::with_capacity(text.len());
    for character in text.chars() {
        if is_terminal_safe_character(character) {
            safe.push(character);
        } else {
            safe.extend(character.escape_default());
        }
    }
    safe
}

fn is_terminal_safe_character(character: char) -> bool {
    !character.is_control() || matches!(character, '\n' | '\r' | '\t')
}
