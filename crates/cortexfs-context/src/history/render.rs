use super::{EMPTY_HISTORY, HistorySelection, RenderedHistory};
use crate::Message;
use crate::render::{clip, render_message};

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

fn warning(max_chars: usize) -> String {
    format!(
        "WARNING: historical messages exceeded the {max_chars} character budget; oldest messages were omitted.\n\n"
    )
}
