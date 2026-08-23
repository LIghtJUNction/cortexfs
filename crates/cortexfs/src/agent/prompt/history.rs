use super::compact::{format_history_with_strategy, read_compact_strategy};
use super::read::read_history_messages_tail;
use crate::runtime::compactabi::CompactInvocation;
use crate::*;
use serde_json::Value;

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

pub(crate) fn collect_history_messages_for_agent(
    session_dir: &Path,
    max_chars: usize,
    control_dir: &Path,
    agent: &str,
    session: &str,
    identity: &AgentUnixIdentity,
) -> String {
    let Ok(messages) = read_history_messages_tail(session_dir) else {
        return "(no historical messages injected)".to_owned();
    };
    let strategy = read_compact_strategy(control_dir);
    let invocation = CompactInvocation {
        agent,
        session,
        max_chars,
    };
    format_history_with_strategy(&messages, strategy, control_dir, &invocation, identity)
}

/// Formats durable JSONL through the publishable context crate.
#[must_use]
pub fn format_history_messages_jsonl(messages: &str, max_chars: usize) -> String {
    cortexfs_context::History::from_jsonl(messages)
        .render(max_chars)
        .text()
        .to_owned()
}

pub(crate) fn message_content_text(content: Option<&Value>) -> String {
    cortexfs_context::content_text(content)
}
