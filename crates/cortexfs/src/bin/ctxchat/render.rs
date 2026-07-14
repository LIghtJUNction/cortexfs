use std::io::{self, Write};

use cortexfs::support::terminal::terminal_safe_text;
use serde_json::Value;

pub(crate) fn frames(frames: &[Value], raw: bool) -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    for frame in frames {
        if raw {
            writeln!(stdout, "{frame}")?;
            continue;
        }
        if frame.get("type").and_then(Value::as_str) == Some("delta") {
            if let Some(text) = frame.get("text").and_then(Value::as_str) {
                write!(stdout, "{}", terminal_safe_text(text))?;
            }
        } else if frame.get("type").and_then(Value::as_str) == Some("message") {
            for text in frame
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|part| part.get("text").and_then(Value::as_str))
            {
                write!(stdout, "{}", terminal_safe_text(text))?;
            }
        } else if frame.get("type").and_then(Value::as_str) == Some("error") {
            writeln!(
                stdout,
                "error: {}",
                terminal_safe_text(
                    frame
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown error")
                )
            )?;
        }
    }
    writeln!(stdout)?;
    stdout.flush()
}

pub(crate) fn clear() -> io::Result<()> {
    let mut stdout = io::stdout().lock();
    stdout.write_all(b"\x1b[2J\x1b[H")?;
    stdout.flush()
}
