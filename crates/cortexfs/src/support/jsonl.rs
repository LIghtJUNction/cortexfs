//! Shared JSONL line iteration and shape parsing.

use serde_json::Value;

/// Structural parse of one non-empty JSONL line.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JsonlLineShape {
    /// Line is not valid JSON.
    InvalidJson,
    /// Line parsed as JSON.
    Value(Value),
}

/// Parses one JSONL line body (caller skips empty lines).
#[must_use]
pub fn parse_jsonl_line(line: &str) -> JsonlLineShape {
    match serde_json::from_str::<Value>(line) {
        Ok(value) => JsonlLineShape::Value(value),
        Err(_error) => JsonlLineShape::InvalidJson,
    }
}

/// Iterates non-empty JSONL lines with 1-based line numbers.
pub fn for_each_jsonl_line(content: &str, mut visit: impl FnMut(usize, &str)) {
    for (index, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        visit(index + 1, line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_iterate_jsonl_lines() {
        assert!(matches!(parse_jsonl_line("{"), JsonlLineShape::InvalidJson));
        assert!(matches!(
            parse_jsonl_line(r#"{"a":1}"#),
            JsonlLineShape::Value(_)
        ));

        let mut seen = Vec::new();
        for_each_jsonl_line("\n{\"a\":1}\n\n{\"b\":2}\n", |line, body| {
            seen.push((line, body.to_owned()));
        });
        assert_eq!(
            seen,
            vec![(2, r#"{"a":1}"#.to_owned()), (4, r#"{"b":2}"#.to_owned())]
        );
    }
}
