//! Shared JSONL line iteration and shape parsing.

use std::io::{BufRead, Error, ErrorKind, Read};

use serde_json::Value;

/// Reads one complete, size-bounded JSONL line body.
pub(crate) fn read_jsonl_line(
    reader: &mut impl BufRead,
    max_bytes: usize,
) -> std::io::Result<Option<String>> {
    let mut bytes = Vec::new();
    let limit = u64::try_from(max_bytes)
        .map_err(|_error| Error::other("JSONL line limit too large"))?
        .checked_add(2)
        .ok_or_else(|| Error::other("JSONL line limit too large"))?;
    let read = reader.by_ref().take(limit).read_until(b'\n', &mut bytes)?;
    if read == 0 {
        return Ok(None);
    }
    if !bytes.ends_with(b"\n") {
        return Err(Error::new(
            ErrorKind::InvalidData,
            "JSONL line is missing its terminating newline",
        ));
    }
    bytes.pop();
    if bytes.len() > max_bytes {
        return Err(Error::new(ErrorKind::InvalidData, "JSONL line too large"));
    }
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|_error| Error::new(ErrorKind::InvalidData, "JSONL line is not UTF-8"))
}

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
    use std::io::Cursor;

    #[test]
    fn reads_complete_jsonl_lines() -> std::io::Result<()> {
        let mut reader = Cursor::new(b"first\nsecond\n");

        assert_eq!(read_jsonl_line(&mut reader, 6)?, Some("first".to_owned()));
        assert_eq!(read_jsonl_line(&mut reader, 6)?, Some("second".to_owned()));
        assert_eq!(read_jsonl_line(&mut reader, 6)?, None);
        Ok(())
    }

    #[test]
    fn refuses_jsonl_line_without_final_newline() {
        let mut reader = Cursor::new(b"incomplete");

        assert!(
            read_jsonl_line(&mut reader, 10)
                .is_err_and(|error| error.kind() == ErrorKind::InvalidData)
        );
    }

    #[test]
    fn refuses_oversized_jsonl_line() {
        let mut reader = Cursor::new(b"large\n");

        assert!(
            read_jsonl_line(&mut reader, 4)
                .is_err_and(|error| error.kind() == ErrorKind::InvalidData)
        );
    }

    #[test]
    fn refuses_non_utf8_jsonl_line() {
        let mut reader = Cursor::new([0xff, b'\n']);

        assert!(
            read_jsonl_line(&mut reader, 1)
                .is_err_and(|error| error.kind() == ErrorKind::InvalidData)
        );
    }

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
