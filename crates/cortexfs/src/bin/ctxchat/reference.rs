use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde_json::Value;

const MAX_REFERENCE_BYTES: usize = 64 * 1024;
const MAX_CONTEXT_BYTES: usize = 128 * 1024;
const MAX_HISTORY_BYTES: u64 = 1024 * 1024;

pub(crate) fn expand(input: &str, workspace: &Path, messages: &Path) -> io::Result<String> {
    let history = history_texts(messages)?;
    let mut blocks = String::new();
    for token in input
        .split_whitespace()
        .filter(|token| token.starts_with('@'))
    {
        let block = if let Some(query) = token.strip_prefix("@history:") {
            history_block(&history, query)?
        } else {
            path_block(workspace, token.trim_start_matches('@'))?
        };
        if blocks.len().saturating_add(block.len()) > MAX_CONTEXT_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "reference context exceeds limit",
            ));
        }
        blocks.push_str(&block);
    }
    if blocks.is_empty() {
        Ok(input.to_owned())
    } else {
        Ok(format!(
            "{input}\n\n<context-references>\n{blocks}</context-references>"
        ))
    }
}

fn path_block(workspace: &Path, value: &str) -> io::Result<String> {
    let root = fs::canonicalize(workspace)?;
    let candidate = root.join(value);
    let metadata = fs::symlink_metadata(&candidate)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "reference cannot be a symlink",
        ));
    }
    let canonical = fs::canonicalize(&candidate)?;
    if !canonical.starts_with(&root) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "reference escapes workspace",
        ));
    }
    if metadata.is_dir() {
        let mut names = fs::read_dir(&canonical)?
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect::<Vec<_>>();
        names.sort();
        return Ok(format!(
            "<reference path={value:?} type=\"directory\">\n{}\n</reference>\n",
            names.join("\n")
        ));
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(&canonical)?;
    let max_bytes = u64::try_from(MAX_REFERENCE_BYTES).unwrap_or(u64::MAX);
    if file.metadata()?.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reference file is too large",
        ));
    }
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
        .read_to_end(&mut bytes)?;
    let text = String::from_utf8(bytes)
        .map_err(|_error| io::Error::new(io::ErrorKind::InvalidData, "reference file is binary"))?;
    Ok(format!(
        "<reference path={value:?} type=\"file\">\n{text}\n</reference>\n"
    ))
}

fn history_block(history: &[String], query: &str) -> io::Result<String> {
    let selected = query
        .parse::<usize>()
        .ok()
        .and_then(|index| history.get(index))
        .or_else(|| history.iter().rev().find(|text| text.contains(query)))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "history reference not found"))?;
    Ok(format!(
        "<reference path={query:?} type=\"history\">\n{selected}\n</reference>\n"
    ))
}

pub(crate) fn history_texts(path: &Path) -> io::Result<Vec<String>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "history is not a plain file",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    if metadata.len() > MAX_HISTORY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "history is too large",
        ));
    }
    let text = fs::read_to_string(path)?;
    Ok(text
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|value| {
            value
                .get("content")?
                .as_array()?
                .iter()
                .find_map(|part| part.get("text").and_then(Value::as_str))
                .map(str::to_owned)
        })
        .collect())
}

pub(crate) fn complete_paths(workspace: &Path, prefix: &str) -> Vec<String> {
    let candidate = workspace.join(prefix);
    let (parent, stem) = if candidate.is_dir() {
        (candidate, "")
    } else {
        (
            candidate.parent().unwrap_or(workspace).to_path_buf(),
            candidate.file_name().and_then(|v| v.to_str()).unwrap_or(""),
        )
    };
    let base = Path::new(prefix)
        .parent()
        .and_then(Path::to_str)
        .unwrap_or("");
    fs::read_dir(parent)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|name| name.starts_with(stem))
        .map(|name| {
            if base.is_empty() {
                name
            } else {
                format!("{base}/{name}")
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn references_preserve_multiline_and_reject_escape_and_binary() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        fs::write(root.path().join("note.txt"), "one\ntwo\n")?;
        fs::write(root.path().join("binary"), [0xff, 0xfe])?;
        let outside = root.path().join("outside");
        fs::write(&outside, "secret")?;
        symlink(&outside, root.path().join("link"))?;
        let expanded = expand(
            "review @note.txt",
            root.path(),
            &root.path().join("missing"),
        )?;
        assert!(expanded.contains("one\ntwo"));
        assert!(expand("@binary", root.path(), &root.path().join("missing")).is_err());
        assert!(expand("@link", root.path(), &root.path().join("missing")).is_err());
        Ok(())
    }

    #[test]
    fn history_reference_supports_index_and_query() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let messages = root.path().join("messages.jsonl");
        fs::write(
            &messages,
            concat!(
                "{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"alpha\"}]}\n",
                "{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"beta result\"}]}\n"
            ),
        )?;
        assert!(expand("@history:0", root.path(), &messages)?.contains("alpha"));
        assert!(expand("@history:beta", root.path(), &messages)?.contains("beta result"));
        Ok(())
    }

    #[test]
    fn nested_path_completion_keeps_directory_prefix() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        fs::create_dir_all(root.path().join("src"))?;
        fs::write(root.path().join("src/main.rs"), "fn main() {}")?;
        assert_eq!(complete_paths(root.path(), "src/ma"), vec!["src/main.rs"]);
        Ok(())
    }
}
