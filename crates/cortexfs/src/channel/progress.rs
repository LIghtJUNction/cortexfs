pub(super) fn fits(text: &str, max_chars: usize) -> bool {
    text.chars().count() <= max_chars
}

pub(super) fn append_bounded(output: &mut String, text: &str, max_bytes: usize) {
    for character in text.chars() {
        if output.len().saturating_add(character.len_utf8()) > max_bytes {
            break;
        }
        output.push(character);
    }
}
