use crate::*;

pub(crate) fn waiting_diagnostic(seconds: u64) -> String {
    let color = color_enabled();
    format!(
        "{} {} {}",
        styled(color, ANSI_DIM, "agent"),
        styled(color, ANSI_CYAN, &format!("waiting {seconds}s")),
        styled(color, ANSI_DIM, "for first event...")
    )
}

pub(crate) fn debug_timing_diagnostic(value: &serde_json::Value) -> Option<String> {
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

const TOOL_ARGUMENT_PREVIEW_CHARS: usize = 180;
const TOOL_RESULT_PREVIEW_CHARS: usize = 360;
const TOOL_RESULT_PREVIEW_LINES: usize = 6;

pub(crate) fn tool_running_diagnostic(value: &serde_json::Value) -> String {
    let color = color_enabled();
    let name = terminal_safe_text(
        value
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("tool_call"),
    );
    let header = format!(
        "{} {} {}",
        styled(color, ANSI_BOLD_YELLOW, "tool"),
        styled(color, ANSI_CYAN, &name),
        styled(color, ANSI_DIM, "running")
    );
    if let Some(args) = tool_arguments_summary(value) {
        format!("{header} {}", styled(color, ANSI_DIM, &args))
    } else {
        header
    }
}

pub(crate) fn tool_result_diagnostic(value: &serde_json::Value) -> String {
    let color = color_enabled();
    let name = value
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("tool");
    let name = terminal_safe_text(name);
    let bytes = tool_message_content_bytes(value);
    let tokens = estimated_tokens_from_bytes(bytes);
    let mut diagnostic = format!(
        "{} {} {} {}",
        styled(color, ANSI_BOLD_YELLOW, "tool"),
        styled(color, ANSI_CYAN, &name),
        styled(color, ANSI_GREEN, "done"),
        styled(color, ANSI_DIM, &format!("{bytes} bytes ~{tokens} tokens"))
    );
    if let Some(args) = tool_arguments_summary(value) {
        diagnostic.push('\n');
        diagnostic.push_str(&styled(color, ANSI_DIM, &format!("  args: {args}")));
    }
    if let Some(result) = tool_result_preview(value) {
        diagnostic.push('\n');
        diagnostic.push_str(&styled(color, ANSI_DIM, &format!("  result: {result}")));
    }
    diagnostic
}

pub(crate) fn tool_message_content_bytes(value: &serde_json::Value) -> usize {
    match value.get("content") {
        Some(content) if content.is_string() => content.as_str().map_or(0, str::len),
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

pub(crate) fn estimated_tokens_from_bytes(bytes: usize) -> usize {
    bytes.div_ceil(4)
}

pub(crate) fn tool_arguments_summary(value: &serde_json::Value) -> Option<String> {
    let args = value
        .get("arguments")?
        .get("args")
        .and_then(serde_json::Value::as_array)?;
    if args.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    for (index, arg) in args.iter().enumerate() {
        let Some(text) = arg.as_str() else {
            continue;
        };
        if index == 0 {
            parts.push(terminal_safe_text(text));
        } else {
            parts.push(terminal_safe_text(
                &serde_json::to_string(text).unwrap_or_else(|_error| format!("{text:?}")),
            ));
        }
    }
    (!parts.is_empty()).then(|| truncate_preview(&parts.join(" "), TOOL_ARGUMENT_PREVIEW_CHARS))
}

pub(crate) fn tool_result_preview(value: &serde_json::Value) -> Option<String> {
    let text = tool_result_text(value)?;
    let preview =
        truncate_multiline_preview(&text, TOOL_RESULT_PREVIEW_CHARS, TOOL_RESULT_PREVIEW_LINES);
    (!preview.is_empty()).then_some(preview)
}

pub(crate) fn tool_result_text(value: &serde_json::Value) -> Option<String> {
    let content = value.get("content")?;
    if let Some(text) = content.as_str() {
        return Some(text.to_owned());
    }
    let items = content.as_array()?;
    let mut parts = Vec::new();
    for item in items {
        if let Some(text) = item
            .get("content")
            .or_else(|| item.get("text"))
            .and_then(serde_json::Value::as_str)
        {
            parts.push(text);
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

pub(crate) fn truncate_multiline_preview(text: &str, max_chars: usize, max_lines: usize) -> String {
    let mut preview = String::new();
    let mut truncated = false;
    for (index, line) in text.lines().enumerate() {
        if index >= max_lines {
            truncated = true;
            break;
        }
        if !preview.is_empty() {
            preview.push('\n');
        }
        preview.push_str(line);
    }
    if preview.len() < text.len() && text.lines().count() > max_lines {
        truncated = true;
    }
    let preview = truncate_preview(&preview, max_chars);
    if truncated && !preview.ends_with("...") {
        format!("{preview}...")
    } else {
        preview
    }
}

pub(crate) fn truncate_preview(text: &str, max_chars: usize) -> String {
    let safe = terminal_safe_text(text);
    if safe.chars().count() <= max_chars {
        return safe;
    }
    let mut truncated = safe
        .chars()
        .take(max_chars.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

pub(crate) fn error_diagnostic(code: &str, message: &str) -> String {
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
pub(crate) fn push_buffered_output(output: &mut String, text: &str) -> Result<(), CliError> {
    let bytes = output
        .len()
        .checked_add(text.len())
        .ok_or_else(|| CliError::unavailable("agent output exceeds buffered output limit"))?;
    if bytes > MAX_BUFFERED_AGENT_RENDERED_BYTES {
        return Err(CliError::unavailable(format!(
            "agent output exceeds {MAX_BUFFERED_AGENT_RENDERED_BYTES} buffered bytes"
        )));
    }
    output.push_str(text);
    Ok(())
}

#[cfg(test)]
pub(crate) fn push_buffered_diagnostic(
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

pub(crate) fn json_text_field(value: &serde_json::Value) -> Option<&str> {
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

pub(crate) fn print_terminal_text(text: &str) -> Result<(), CliError> {
    let text = terminal_safe_text(text);
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.flush())
        .map_err(|error| CliError::unavailable(format!("stdout write failed: {error}")))
}

pub(crate) fn print_terminal_line(line: &str) -> Result<(), CliError> {
    let line = terminal_safe_text(line);
    print_line(&line)
}

pub(crate) fn write_terminal_diagnostic(line: &str) -> Result<(), CliError> {
    write_error(line)
        .map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))
}

pub(crate) fn write_terminal_status(line: &str) -> Result<(), CliError> {
    let mut stderr = io::stderr().lock();
    stderr
        .write_all(b"\r\x1b[2K")
        .and_then(|()| stderr.write_all(line.as_bytes()))
        .and_then(|()| stderr.flush())
        .map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))
}

pub(crate) fn clear_terminal_status() -> Result<(), CliError> {
    let mut stderr = io::stderr().lock();
    stderr
        .write_all(b"\r\x1b[2K")
        .and_then(|()| stderr.flush())
        .map_err(|error| CliError::unavailable(format!("stderr write failed: {error}")))
}

pub(crate) fn terminal_safe_field(text: &str) -> String {
    terminal_safe_text(text)
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}
