#![expect(
    clippy::redundant_pub_crate,
    reason = "Internal helpers must not become unreachable pub API"
)]

use crate::Message;

pub(super) fn render_message(message: &Message) -> String {
    format!("- {}: {}", message.role(), message.content().trim())
}

pub(super) fn clip(value: &str, max_chars: usize) -> String {
    if value.len() <= max_chars {
        return value.to_owned();
    }
    let mut end = max_chars;
    while !value.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    value.get(..end).unwrap_or_default().to_owned()
}
