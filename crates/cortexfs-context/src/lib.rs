#![forbid(unsafe_code)]

//! Rebuildable context history, budgets, and compaction primitives.

mod budget;
mod compact;
mod history;
mod message;

pub use budget::ContextBudget;
pub use compact::{CompactedHistory, Summarizer, compact_history};
pub use history::{History, HistorySelection, RenderedHistory};
pub use message::{Message, content_text, message_from_json_line};

/// Default bounded history slice used when a model window is unknown.
pub const DEFAULT_HISTORY_CHARS: usize = 8_000;
