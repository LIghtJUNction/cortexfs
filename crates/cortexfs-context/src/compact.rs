use crate::render::{clip, render_message};
use crate::{History, HistorySelection, Message};

/// Built-in summarizer that joins omitted messages as bullet lines.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DefaultSummarizer;

impl Summarizer for DefaultSummarizer {
    type Error = std::convert::Infallible;

    fn summarize(&self, messages: &[Message]) -> Result<String, Self::Error> {
        Ok(messages
            .iter()
            .map(render_message)
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

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
    let omitted = history
        .messages()
        .get(..selection.omitted())
        .unwrap_or_default();
    let summary = summarizer
        .filter(|_| !omitted.is_empty())
        .map(|summarizer| summarizer.summarize(omitted))
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

/// Renders a selected history with an optional summary under a byte budget.
#[must_use]
pub fn render_selection(
    selection: &HistorySelection,
    max_chars: usize,
    summary: Option<&str>,
) -> CompactedHistory {
    let recent = selection
        .messages()
        .iter()
        .map(render_message)
        .collect::<Vec<_>>()
        .join("\n");
    let recent = clip(&recent, max_chars);
    // A replaceable compactor may return an arbitrarily long summary. It must
    // never overwrite the newest observations or exceed the shared byte limit.
    let heading = "Summary of earlier context:\n";
    let available = max_chars.saturating_sub(recent.len() + heading.len() + 2);
    let summary = summary
        .map(|value| clip(value.trim(), available))
        .unwrap_or_default();
    let summarized = !summary.is_empty();
    let text = if summarized {
        format!("{heading}{summary}\n\n{recent}")
    } else {
        recent
    };
    CompactedHistory {
        text,
        omitted: selection.omitted(),
        summarized,
    }
}
