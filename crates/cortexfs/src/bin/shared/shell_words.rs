macro_rules! parse_shell_words {
    ($value:expr, $unfinished_escape:expr, $unterminated_quote:expr) => {{
        let mut words = Vec::new();
        let mut word = String::new();
        let mut quote = None;
        let mut escape = false;
        for character in $value.chars() {
            if escape {
                word.push(character);
                escape = false;
                continue;
            }
            match (quote, character) {
                (_, '\\') => escape = true,
                (Some(active), candidate) if candidate == active => quote = None,
                (Some(_active), candidate) => word.push(candidate),
                (None, '\'' | '"') => quote = Some(character),
                (None, candidate) if candidate.is_whitespace() => {
                    if !word.is_empty() {
                        words.push(std::mem::take(&mut word));
                    }
                }
                (None, candidate) => word.push(candidate),
            }
        }
        if escape {
            Err($unfinished_escape)
        } else if quote.is_some() {
            Err($unterminated_quote)
        } else {
            if !word.is_empty() {
                words.push(word);
            }
            Ok(words)
        }
    }};
}
