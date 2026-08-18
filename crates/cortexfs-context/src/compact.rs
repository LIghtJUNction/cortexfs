use crate::{History, HistorySelection, Message};

/// Provider-independent summary callback for older context.
pub trait Summarizer {
    /// Error returned by the selected summary implementation.
    type Error;

    /// Summarizes messages that would otherwise be omitted.
    fn summarize(&self, messages: &[Message]) -> Result<String, Self::Error>;
}

/// The rebuildable result of one context compaction pass.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompactedHistory {
    text: String,
    omitted: usize,
    summarized: bool,
}

/// Compacts history by retaining the newest messages and optionally summarizing older ones.
pub fn compact_history<S: Summarizer>(
    history: &History,
    max_chars: usize,
    summarizer: Option<&S>,
) -> Result<CompactedHistory, S::Error> {
    let selection = history.select(max_chars);
    if selection.omitted() == 0 || summarizer.is_none() {
        return Ok(render_selection(&selection, max_chars, None));
    }
    let omitted = history
        .messages()
        .get(..selection.omitted())
        .unwrap_or_default();
    let summary = summarizer
        .map(|value| value.summarize(omitted))
        .transpose()?;
    Ok(render_selection(&selection, max_chars, summary.as_deref()))
}

impl CompactedHistory {
    /// Returns the compacted text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the number of omitted durable messages.
    #[must_use]
    pub const fn omitted(&self) -> usize {
        self.omitted
    }

    /// Returns whether an external summary was inserted.
    #[must_use]
    pub const fn summarized(&self) -> bool {
        self.summarized
    }
}

fn render_selection(
    selection: &HistorySelection,
    max_chars: usize,
    summary: Option<&str>,
) -> CompactedHistory {
    let recent = selection
        .messages()
        .iter()
        .map(|message| format!("- {}: {}", message.role(), message.content().trim()))
        .collect::<Vec<_>>()
        .join("\n");
    let prefix = summary
        .map(|value| format!("Summary of earlier context:\n{}\n\n", value.trim()))
        .unwrap_or_default();
    let text = clip(&format!("{prefix}{recent}"), max_chars);
    CompactedHistory {
        text,
        omitted: selection.omitted(),
        summarized: summary.is_some(),
    }
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
