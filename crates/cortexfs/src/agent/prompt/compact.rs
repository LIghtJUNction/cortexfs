use std::path::Path;

use cortexfs_context::{DefaultSummarizer, History, compact_history};

use crate::AgentUnixIdentity;
use crate::agent::compactstrategy::CompactStrategy;
use crate::runtime::compactabi::CompactInvocation;
use crate::runtime::compactexec::run_custom_compact;
use crate::support::plain::read_small_text_file;

const MAX_STRATEGY_BYTES: u64 = 256;

/// Reads `agent/<name>.d/compact.strategy`, defaulting to truncate.
#[must_use]
pub fn read_compact_strategy(control_dir: &Path) -> CompactStrategy {
    match read_small_text_file(&control_dir.join("compact.strategy"), MAX_STRATEGY_BYTES) {
        Ok(content) => CompactStrategy::parse(&content).unwrap_or_default(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => CompactStrategy::default(),
        Err(_error) => CompactStrategy::default(),
    }
}

/// Rebuilds bounded prompt history using the selected compaction strategy.
#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "history compaction keeps strategy, control dir, and session identity explicit"
)]
pub fn format_history_with_strategy(
    messages: &str,
    max_chars: usize,
    strategy: CompactStrategy,
    control_dir: &Path,
    agent: &str,
    session: &str,
    identity: &AgentUnixIdentity,
) -> String {
    let history = History::from_jsonl(messages);
    match strategy {
        CompactStrategy::Truncate => history.render(max_chars).text().to_owned(),
        CompactStrategy::Summarize => compact_with_builtin(&history, max_chars),
        CompactStrategy::Custom(name) => {
            let path = control_dir.join("compact.d").join(&name);
            compact_with_custom(&history, max_chars, &path, agent, session, identity)
        }
    }
}

fn compact_with_builtin(history: &History, max_chars: usize) -> String {
    match compact_history(history, max_chars, Some(&DefaultSummarizer)) {
        Ok(compacted) => compacted.text().to_owned(),
        Err(error) => match error {},
    }
}

fn compact_with_custom(
    history: &History,
    max_chars: usize,
    path: &Path,
    agent: &str,
    session: &str,
    identity: &AgentUnixIdentity,
) -> String {
    let selection = history.select(max_chars);
    if selection.omitted() == 0 {
        return selection.render(max_chars).text().to_owned();
    }
    let omitted = history
        .messages()
        .get(..selection.omitted())
        .unwrap_or_default();
    let invocation = CompactInvocation {
        agent,
        session,
        max_chars,
    };
    match run_custom_compact(path, &invocation, omitted, identity) {
        Ok(summary) => render_with_summary(&selection, max_chars, Some(&summary)),
        Err(_error) => history.render(max_chars).text().to_owned(),
    }
}

fn render_with_summary(
    selection: &cortexfs_context::HistorySelection,
    max_chars: usize,
    summary: Option<&str>,
) -> String {
    let recent = selection
        .messages()
        .iter()
        .map(|message| format!("- {}: {}", message.role(), message.content().trim()))
        .collect::<Vec<_>>()
        .join("\n");
    let prefix = summary
        .map(|value| format!("Summary of earlier context:\n{}\n\n", value.trim()))
        .unwrap_or_default();
    clip(&format!("{prefix}{recent}"), max_chars)
}

fn clip(value: &str, max_chars: usize) -> String {
    if value.len() <= max_chars {
        return value.to_owned();
    }
    let marker = "\n[context truncated]\n";
    let mut end = max_chars.saturating_sub(marker.len());
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    format!("{}{marker}", value.get(..end).unwrap_or_default())
}
