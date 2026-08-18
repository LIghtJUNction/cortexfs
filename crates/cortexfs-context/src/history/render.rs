use super::{EMPTY_HISTORY, HistorySelection, RenderedHistory};
use crate::message::Message;

impl HistorySelection {
    /// Returns selected messages in durable order.
    #[must_use]
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Returns how many older messages were omitted.
    #[must_use]
    pub const fn omitted(&self) -> usize {
        self.omitted
    }

    /// Renders the selection, including an explicit omission warning.
    #[must_use]
    pub fn render(&self, max_chars: usize) -> RenderedHistory {
        let lines = self.messages.iter().map(render_message).collect::<Vec<_>>();
        let mut text = lines.join("\n");
        if self.omitted > 0 {
            text = clip(&format!("{text}\n{}", warning(max_chars)), max_chars);
        }
        if text.is_empty() {
            EMPTY_HISTORY.clone_into(&mut text);
        }
        RenderedHistory {
            text,
            omitted: self.omitted,
        }
    }
}

impl RenderedHistory {
    /// Returns the bounded rendered history text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns how many older messages were omitted.
    #[must_use]
    pub const fn omitted(&self) -> usize {
        self.omitted
    }
}

pub(super) fn render_message(message: &Message) -> String {
    format!("- {}: {}", message.role(), message.content().trim())
}

fn warning(max_chars: usize) -> String {
    format!(
        "WARNING: historical messages exceeded the {max_chars} character budget; oldest messages were omitted.\n\n"
    )
}

fn clip(value: &str, max_chars: usize) -> String {
    if value.len() <= max_chars {
        return value.to_owned();
    }
    let mut end = max_chars;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value.get(..end).unwrap_or_default().to_owned()
}
