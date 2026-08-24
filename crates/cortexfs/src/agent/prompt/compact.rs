use std::path::Path;

use cortexfs_context::{DefaultSummarizer, History, compact_history, render_selection};

use crate::AgentUnixIdentity;
use crate::agent::compactstrategy::CompactStrategy;
use crate::runtime::compactabi::CompactInvocation;
use crate::runtime::run_custom_compact;
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
pub fn format_history_with_strategy(
    messages: &str,
    strategy: CompactStrategy,
    control_dir: &Path,
    invocation: &CompactInvocation<'_>,
    identity: &AgentUnixIdentity,
) -> String {
    let history = History::from_jsonl(messages);
    match strategy {
        CompactStrategy::Truncate => history.render(invocation.max_chars).text().to_owned(),
        CompactStrategy::Summarize => {
            compact_history(&history, invocation.max_chars, Some(&DefaultSummarizer))
                .unwrap_or_else(|error| match error {})
                .text()
                .to_owned()
        }
        CompactStrategy::Custom(name) => {
            let path = control_dir.join("compact.d").join(&name);
            compact_with_custom(&history, &path, invocation, identity)
        }
    }
}

fn compact_with_custom(
    history: &History,
    path: &Path,
    invocation: &CompactInvocation<'_>,
    identity: &AgentUnixIdentity,
) -> String {
    let selection = history.select(invocation.max_chars);
    if selection.omitted() == 0 {
        return selection.render(invocation.max_chars).text().to_owned();
    }
    let omitted = history
        .messages()
        .get(..selection.omitted())
        .unwrap_or_default();
    match run_custom_compact(path, invocation, omitted, identity) {
        Ok(summary) => render_selection(&selection, invocation.max_chars, Some(&summary))
            .text()
            .to_owned(),
        Err(_error) => history.render(invocation.max_chars).text().to_owned(),
    }
}
