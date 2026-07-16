use crate::*;

use std::collections::BTreeSet;

const MAX_FIND_BYTES: usize = 1024;
const MAX_FIND_TERMS: usize = 16;
const MAX_FIND_RESULTS: usize = 12;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FindEntry {
    pub(crate) name: String,
    pub(crate) path: PathBuf,
    pub(crate) description: String,
    score: u32,
}

pub(crate) fn repl_find(root: &Path, words: &[String]) -> Result<(), TshError> {
    let entries = find_tools(root, words.get(1..).unwrap_or_default())?;
    let mut stdout = io::stdout().lock();
    for entry in entries {
        writeln!(
            stdout,
            "{}\t{}",
            terminal_safe_text(&entry.name),
            terminal_safe_text(&entry.description)
        )
        .map_err(|error| write_error_to_tsh(&error))?;
    }
    stdout.flush().map_err(|error| write_error_to_tsh(&error))
}

pub(crate) fn find_tools(root: &Path, query: &[String]) -> Result<Vec<FindEntry>, TshError> {
    find_in_path(&ctx_tool_path(root)?, query)
}

fn find_in_path(path: &ToolPath, query: &[String]) -> Result<Vec<FindEntry>, TshError> {
    validate_query(query)?;
    let terms = query
        .iter()
        .map(|term| term.to_lowercase())
        .collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut seen = BTreeSet::new();
    for hit in path
        .list_limited(MAX_TSH_TOOL_COUNT, MAX_TSH_TOOL_COUNT)
        .map_err(tool_path_error)?
    {
        let Some(name) = hit.path().file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !seen.insert(name.to_owned()) {
            continue;
        }
        let description = read_control_text(&hit, "description").unwrap_or_default();
        let schema = read_control_text(&hit, "schema").unwrap_or_default();
        let score = relevance(name, &description, &schema, &terms);
        if score > 0 {
            entries.push(FindEntry {
                name: name.to_owned(),
                path: hit.path().to_path_buf(),
                description,
                score,
            });
        }
    }
    entries.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.path.cmp(&right.path))
    });
    entries.truncate(MAX_FIND_RESULTS);
    Ok(entries)
}

fn validate_query(query: &[String]) -> Result<(), TshError> {
    let bytes = query
        .iter()
        .map(String::len)
        .sum::<usize>()
        .saturating_add(query.len().saturating_sub(1));
    if query.is_empty() || query.len() > MAX_FIND_TERMS || bytes > MAX_FIND_BYTES {
        return Err(TshError::usage(
            "find requires 1..16 query terms within 1024 bytes",
        ));
    }
    if query
        .iter()
        .any(|term| term.is_empty() || term.chars().any(char::is_control))
    {
        return Err(TshError::usage(
            "find query contains an empty or control term",
        ));
    }
    Ok(())
}

fn relevance(name: &str, description: &str, schema: &str, terms: &[String]) -> u32 {
    let lower = name.to_lowercase();
    let segments = lower.split('.').collect::<Vec<_>>();
    let description = description.to_lowercase();
    let schema = schema_text(schema);
    terms
        .iter()
        .map(|term| {
            if lower == *term {
                10_000
            } else if lower.starts_with(term) {
                5_000
            } else if segments.contains(&term.as_str()) {
                3_000
            } else if lower
                .split(|character: char| !character.is_alphanumeric())
                .any(|token| token == term)
            {
                2_000
            } else if lower.contains(term) {
                1_000
            } else if description.contains(term) {
                100
            } else if schema.contains(term) {
                10
            } else {
                0
            }
        })
        .sum()
}

fn schema_text(schema: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(schema) else {
        return String::new();
    };
    let mut text = String::new();
    for key in ["title", "description"] {
        if let Some(value) = value.get(key).and_then(Value::as_str) {
            text.push_str(value);
            text.push(' ');
        }
    }
    if let Some(properties) = value.get("properties").and_then(Value::as_object) {
        for (name, property) in properties {
            text.push_str(name);
            text.push(' ');
            for key in ["title", "description"] {
                if let Some(value) = property.get(key).and_then(Value::as_str) {
                    text.push_str(value);
                    text.push(' ');
                }
            }
        }
    }
    text.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn ranking_prefers_exact_prefix_segment_and_name_over_metadata() {
        let terms = ["read".to_owned()];
        assert!(relevance("read", "", "", &terms) > relevance("reader", "", "", &terms));
        assert!(relevance("reader", "", "", &terms) > relevance("fs.read", "", "", &terms));
        assert!(relevance("fs.read", "", "", &terms) > relevance("fs.other", "read", "", &terms));
        assert!(
            relevance("fs.other", "read", "", &terms)
                > relevance("fs.other", "", r#"{"title":"read"}"#, &terms)
        );
    }

    #[test]
    fn invalid_queries_are_rejected() {
        assert!(validate_query(&[]).is_err());
        assert!(validate_query(&["bad\u{1b}".to_owned()]).is_err());
        assert!(validate_query(&["x".repeat(MAX_FIND_BYTES + 1)]).is_err());
    }

    #[test]
    fn search_crosses_groups_escapes_output_and_does_not_change_context()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let directory = root.path().join("tool");
        fs::create_dir_all(&directory)?;
        for (name, description, schema) in [
            (
                "fs.read",
                "Read files\u{1b}[2J",
                r#"{"type":"object","properties":{"path":{"description":"filesystem path"}}}"#,
            ),
            (
                "github.search",
                "Search issues",
                r#"{"type":"object","title":"remote query"}"#,
            ),
        ] {
            let executable = directory.join(name);
            fs::write(&executable, "#!/bin/sh\n")?;
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
            let control = directory.join(format!("{name}.d"));
            fs::create_dir_all(&control)?;
            fs::write(control.join("description"), description)?;
            fs::write(control.join("schema"), schema)?;
        }
        let context = ToolContext::new(4);
        let before = context.to_state();
        let found = find_tools(root.path(), &["path".to_owned()])
            .map_err(|error| io::Error::other(error.message))?;
        assert_eq!(
            found.first().map(|entry| entry.name.as_str()),
            Some("fs.read")
        );
        assert!(
            terminal_safe_text(
                found
                    .first()
                    .map(|entry| entry.description.as_str())
                    .unwrap_or_default()
            )
            .contains("\\u{1b}")
        );
        assert_eq!(context.to_state(), before);
        assert!(
            find_tools(root.path(), &["search".to_owned()])
                .map_err(|error| io::Error::other(error.message))?
                .iter()
                .any(|entry| entry.name == "github.search")
        );
        let loaded = load_tool_context(root.path(), "github.search", false)
            .map_err(|error| io::Error::other(error.message))?;
        assert_eq!(loaded.name, "github.search");
        assert!(loaded.schema.is_some());
        Ok(())
    }

    #[test]
    fn search_uses_only_the_first_ctx_path_hit_for_shadowed_tools()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let first = root.path().join("first");
        let second = root.path().join("second");
        for (directory, description) in [(&first, "first tier"), (&second, "lower-only-marker")] {
            fs::create_dir_all(directory)?;
            let executable = directory.join("demo.echo");
            fs::write(&executable, "#!/bin/sh\n")?;
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
            let control = directory.join("demo.echo.d");
            fs::create_dir_all(&control)?;
            fs::write(control.join("description"), description)?;
            fs::write(control.join("schema"), r#"{"type":"object"}"#)?;
        }
        let path = format!("{}:{}", first.display(), second.display());
        let home = root.path().join("home");
        let tools = ctx_tool_path_with_home(root.path(), &home, Ok(path), false)
            .map_err(|error| io::Error::other(error.message))?;
        let found = find_in_path(&tools, &["demo.echo".to_owned()])
            .map_err(|error| io::Error::other(error.message))?;
        assert_eq!(found.len(), 1);
        let found = found
            .first()
            .ok_or_else(|| io::Error::other("shadowed tool missing"))?;
        assert_eq!(found.path, first.join("demo.echo"));
        assert_eq!(found.description, "first tier");
        assert!(
            find_in_path(&tools, &["lower-only-marker".to_owned()])
                .map_err(|error| io::Error::other(error.message))?
                .is_empty()
        );
        Ok(())
    }
}
