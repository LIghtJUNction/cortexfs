/// Escapes terminal control characters while preserving ordinary whitespace.
#[must_use]
pub fn terminal_safe_text(text: &str) -> String {
    let mut safe = String::with_capacity(text.len());
    for character in text.chars() {
        if !character.is_control() || matches!(character, '\n' | '\r' | '\t') {
            safe.push(character);
        } else {
            safe.extend(character.escape_default());
        }
    }
    safe
}
