use super::{MAX_AGENT_EXECUTABLE_STDERR_BYTES, Value, agent_terminal_error_frames};
use std::io::Read;

pub(super) const MAX_AGENT_PROCESS_DIAGNOSTIC_CHARS: usize = 512;
const SECRET_MARKERS: [&str; 10] = [
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

pub(super) fn read_agent_executable_stderr_limited(stderr: impl Read) -> std::io::Result<String> {
    let limit = MAX_AGENT_EXECUTABLE_STDERR_BYTES;
    let mut bytes = Vec::new();
    stderr
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    bytes.truncate(
        bytes
            .len()
            .min(usize::try_from(limit).unwrap_or(usize::MAX)),
    );
    Ok(String::from_utf8_lossy(&bytes).trim().to_owned())
}

pub(super) fn terminal_error_message(run: &str, frames: &[String]) -> Option<String> {
    frames.iter().rev().find_map(|frame| {
        let value = serde_json::from_str::<Value>(frame).ok()?;
        (value.get("type").and_then(Value::as_str) == Some("error")
            && value.get("run").and_then(Value::as_str) == Some(run)
            && value.get("recoverable").and_then(Value::as_bool) != Some(true))
        .then(|| {
            value
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .flatten()
    })
}

pub(super) fn safe_agent_process_diagnostic(stderr: &str) -> String {
    let line = stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .map(str::trim)
        .unwrap_or_default();
    let mut diagnostic = line
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>();
    for marker in SECRET_MARKERS {
        diagnostic = redact(&diagnostic, marker);
    }
    diagnostic
        .chars()
        .take(MAX_AGENT_PROCESS_DIAGNOSTIC_CHARS)
        .collect()
}

fn redact(value: &str, marker: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut remaining = value;
    while let Some(start) = remaining.find(marker) {
        let value_start = start.saturating_add(marker.len());
        let (Some(prefix), Some(tail)) =
            (remaining.get(..value_start), remaining.get(value_start..))
        else {
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

pub(super) fn agent_process_failed_frames(run: &str, stderr: &str) -> Vec<String> {
    let diagnostic = safe_agent_process_diagnostic(stderr);
    let message = if diagnostic.is_empty() {
        "agent process failed".to_owned()
    } else {
        format!("agent process failed: {diagnostic}")
    };
    agent_terminal_error_frames(run, "EIO", &message)
}

#[cfg(test)]
mod tests;
