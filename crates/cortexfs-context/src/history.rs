use crate::message::{Message, message_from_json_line};
use crate::render::{clip, render_message};

mod render;

const MAX_MESSAGE_LINE_BYTES: usize = 64 * 1024;
const EMPTY_HISTORY: &str = "(no historical messages injected)";

/// In-memory normalized view of durable session messages.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct History {
    messages: Vec<Message>,
}

/// The newest messages that fit a byte budget, with the last message clipped if necessary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistorySelection {
    messages: Vec<Message>,
    omitted: usize,
}

/// Rendered history plus its omission count.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RenderedHistory {
    text: String,
    omitted: usize,
}

impl History {
    /// Parses valid JSONL messages up to 64 KiB per line, ignoring unrelated or oversized lines.
    #[must_use]
    pub fn from_jsonl(input: &str) -> Self {
        let messages = input
            .lines()
            .filter(|line| line.len() <= MAX_MESSAGE_LINE_BYTES)
            .filter_map(message_from_json_line)
            .collect();
        Self { messages }
    }

    /// Creates a history from normalized messages.
    #[must_use]
    pub fn from_messages(messages: impl IntoIterator<Item = Message>) -> Self {
        Self {
            messages: messages.into_iter().collect(),
        }
    }

    /// Appends one normalized message.
    pub fn push(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Returns all normalized messages in durable order.
    #[must_use]
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Selects recent messages, retaining a marked excerpt if the newest exceeds `max_chars`.
    #[must_use]
    pub fn select(&self, max_chars: usize) -> HistorySelection {
        let mut selected = Vec::new();
        let mut used = 0_usize;
        for message in self.messages.iter().rev() {
            let line = render_message(message);
            let needed = line.len() + usize::from(!selected.is_empty());
            if used.saturating_add(needed) > max_chars {
                let marker = " [truncated]";
                let available = max_chars.saturating_sub(message.role().len() + 4 + marker.len());
                if selected.is_empty() && available > 0 {
                    let excerpt = clip(message.content().trim(), available);
                    selected.push(Message::new(message.role(), format!("{excerpt}{marker}")));
                }
                break;
            }
            used = used.saturating_add(needed);
            selected.push(message.clone());
        }
        selected.reverse();
        HistorySelection {
            omitted: self.messages.len().saturating_sub(selected.len()),
            messages: selected,
        }
    }

    /// Renders recent history without changing the durable history.
    #[must_use]
    pub fn render(&self, max_chars: usize) -> RenderedHistory {
        self.select(max_chars).render(max_chars)
    }
}
