use super::read::{push_str_byte_limit, read_history_messages_tail};
use super::*;
use crate::*;
use std::collections::VecDeque;

#[must_use]
pub fn current_time_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

#[must_use]
pub fn collect_history_messages_from_session(session_dir: &Path, max_chars: usize) -> String {
    let Ok(messages) = read_history_messages_tail(session_dir) else {
        return "(no historical messages injected)".to_owned();
    };
    format_history_messages_jsonl(&messages, max_chars)
}

#[must_use]
pub fn format_history_messages_jsonl(messages: &str, max_chars: usize) -> String {
    let mut rendered = VecDeque::new();
    let mut selected_len = 0;
    let mut truncated = false;

    for line in messages.lines() {
        if line.len() > MAX_HISTORY_MESSAGE_LINE_BYTES {
            continue;
        }
        let Some(line) = render_history_message_line(line) else {
            continue;
        };
        selected_len += line.len() + usize::from(!rendered.is_empty());
        rendered.push_back(line);

        while !rendered.is_empty() && selected_len > max_chars {
            let Some(removed) = rendered.pop_front() else {
                break;
            };
            truncated = true;
            selected_len = selected_len.saturating_sub(removed.len());
            if !rendered.is_empty() {
                selected_len = selected_len.saturating_sub(1);
            }
        }
    }
    if rendered.is_empty() {
        if truncated {
            return clipped_history_budget_warning(max_chars);
        }
        return "(no historical messages injected)".to_owned();
    }
    if !truncated {
        return rendered.into_iter().collect::<Vec<_>>().join("\n");
    }
    fit_history_lines(rendered.into_iter().collect(), max_chars)
}

pub(crate) fn render_history_message_line(line: &str) -> Option<String> {
    let value = serde_json::from_str::<Value>(line).ok()?;
    let role = value.get("role").and_then(Value::as_str)?;
    let text = message_content_text(value.get("content"));
    if text.trim().is_empty() {
        return None;
    }
    Some(format!("- {role}: {}", text.trim()))
}

pub(crate) fn message_content_text(content: Option<&Value>) -> String {
    let Some(content) = content else {
        return String::new();
    };
    if let Some(text) = content.as_str() {
        return text.to_owned();
    }
    if let Some(parts) = content.as_array() {
        return parts
            .iter()
            .filter_map(|part| {
                part.get("text")
                    .and_then(Value::as_str)
                    .or_else(|| part.get("content").and_then(Value::as_str))
            })
            .collect::<Vec<_>>()
            .join("\n");
    }
    content.to_string()
}

pub(crate) fn history_budget_warning(max_chars: usize) -> String {
    format!(
        "WARNING: historical messages exceeded the {max_chars} character budget; oldest messages were omitted.\n\n"
    )
}

pub(crate) fn clipped_history_budget_warning(max_chars: usize) -> String {
    let warning = history_budget_warning(max_chars);
    let mut output = String::new();
    push_str_byte_limit(&mut output, &warning, max_chars);
    output.trim_end().to_owned()
}

pub(crate) fn fit_history_lines(lines: Vec<String>, max_chars: usize) -> String {
    let warning = history_budget_warning(max_chars);
    if warning.len() > max_chars {
        return clipped_history_budget_warning(max_chars);
    }
    let mut selected = Vec::new();
    let mut used = warning.len();
    for line in lines.into_iter().rev() {
        let needed = line.len() + usize::from(!selected.is_empty());
        if used + needed > max_chars {
            break;
        }
        used += needed;
        selected.push(line);
    }
    selected.reverse();
    if selected.is_empty() {
        clipped_history_budget_warning(max_chars)
    } else {
        format!("{warning}{}", selected.join("\n"))
    }
}
