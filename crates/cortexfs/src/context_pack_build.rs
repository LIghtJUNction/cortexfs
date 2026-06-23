use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::{
    CONTEXT_REQUIRED_DIRS, CONTEXT_REQUIRED_FILES, ContextJsonlKind, ContextPackBuild,
    ContextPackBuiltItem, SESSION_REQUIRED_FILES, atomic_replace_text, inspect_context_jsonl,
    inspect_message_stream_jsonl, is_object_name,
};

/// Error while rebuilding an inspectable context pack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextPackBuildError {
    /// Session directory name is not a valid v1 session name.
    InvalidSessionName,
    /// Optional agent name is not a valid v1 object name.
    InvalidAgentName,
    /// Required durable session files or context directory are missing.
    MissingSession,
    /// `context/budget` is not empty or a single unsigned integer value.
    InvalidBudget,
    /// `messages.jsonl` is not valid canonical durable message history.
    InvalidMessages,
    /// A context JSONL source selected for the pack is invalid.
    InvalidContextJsonl,
    /// A child result directory name is not a valid v1 object name.
    InvalidChildName,
    /// Session/context files could not be read.
    CannotRead,
    /// `context/pack.json` or `context/pack.md` could not be written.
    CannotRecord,
}

impl ContextPackBuildError {
    /// Returns a stable errno name for this context pack rebuild failure.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidSessionName
            | Self::InvalidAgentName
            | Self::InvalidBudget
            | Self::InvalidMessages
            | Self::InvalidContextJsonl
            | Self::InvalidChildName => "EINVAL",
            Self::MissingSession => "ENOENT",
            Self::CannotRead | Self::CannotRecord => "EIO",
        }
    }
}

/// Rebuilds derived context pack files for one durable session.
///
/// The generated pack is derived state. It references only session-relative
/// sources that `validate_context_pack_source` accepts, includes recent raw
/// messages by reference, and may include child result channels but never child
/// full-history files.
pub fn rebuild_context_pack(
    session_dir: &Path,
    agent: Option<&str>,
    recent_message_limit: usize,
) -> Result<ContextPackBuild, ContextPackBuildError> {
    let session_name = session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ContextPackBuildError::InvalidSessionName)?;
    if !is_object_name(session_name) {
        return Err(ContextPackBuildError::InvalidSessionName);
    }
    if let Some(agent) = agent
        && !is_object_name(agent)
    {
        return Err(ContextPackBuildError::InvalidAgentName);
    }
    require_pack_session_files(session_dir)?;

    let context = session_dir.join("context");
    let budget = read_context_budget(&context.join("budget"))?;
    let messages = fs::read_to_string(session_dir.join("messages.jsonl"))
        .map_err(|_error| ContextPackBuildError::CannotRead)?;
    if !inspect_message_stream_jsonl(&messages).is_ok() {
        return Err(ContextPackBuildError::InvalidMessages);
    }

    let mut candidates = Vec::new();
    append_pinned_pack_candidates(&context, &mut candidates)?;
    append_context_file_candidate(
        &context,
        "summary",
        "context/summary.md",
        None,
        &mut candidates,
    )?;
    append_context_jsonl_candidate(
        &context,
        "facts",
        "context/facts.jsonl",
        ContextJsonlKind::Facts,
        &mut candidates,
    )?;
    append_context_jsonl_candidate(
        &context,
        "decisions",
        "context/decisions.jsonl",
        ContextJsonlKind::Decisions,
        &mut candidates,
    )?;
    append_context_file_candidate(&context, "todo", "context/todo.md", None, &mut candidates)?;
    append_context_jsonl_candidate(
        &context,
        "refs",
        "context/refs.jsonl",
        ContextJsonlKind::Refs,
        &mut candidates,
    )?;
    append_recent_messages_candidate(&messages, recent_message_limit, &mut candidates);
    append_child_result_candidates(&context, &mut candidates)?;

    let selected = select_pack_candidates(candidates, budget);
    let build = render_context_pack(session_name, agent, budget, &selected);
    if !crate::context_pack_inspect::inspect_context_pack_json(build.pack_json()).is_ok() {
        return Err(ContextPackBuildError::CannotRecord);
    }

    atomic_replace_text(&context.join("pack.json"), build.pack_json())
        .map_err(|_error| ContextPackBuildError::CannotRecord)?;
    atomic_replace_text(&context.join("pack.md"), build.pack_md())
        .map_err(|_error| ContextPackBuildError::CannotRecord)?;

    Ok(build)
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct PackCandidate {
    kind: String,
    source: String,
    range: Option<String>,
    tokens: u64,
    content: String,
}

impl PackCandidate {
    fn new(kind: &str, source: &str, range: Option<String>, content: String) -> Self {
        Self {
            kind: kind.to_owned(),
            source: source.to_owned(),
            range,
            tokens: estimate_context_tokens(&content),
            content,
        }
    }

    fn item(&self) -> ContextPackBuiltItem {
        ContextPackBuiltItem::new(&self.kind, &self.source, self.range.clone(), self.tokens)
    }
}

fn require_pack_session_files(session_dir: &Path) -> Result<(), ContextPackBuildError> {
    for file in SESSION_REQUIRED_FILES {
        if !session_dir.join(file).is_file() {
            return Err(ContextPackBuildError::MissingSession);
        }
    }
    let context = session_dir.join("context");
    if !context.is_dir() {
        return Err(ContextPackBuildError::MissingSession);
    }
    for file in CONTEXT_REQUIRED_FILES {
        if !context.join(file).is_file() {
            return Err(ContextPackBuildError::MissingSession);
        }
    }
    for dir in CONTEXT_REQUIRED_DIRS {
        if !context.join(dir).is_dir() {
            return Err(ContextPackBuildError::MissingSession);
        }
    }
    Ok(())
}

fn read_context_budget(path: &Path) -> Result<Option<u64>, ContextPackBuildError> {
    let content = fs::read_to_string(path).map_err(|_error| ContextPackBuildError::CannotRead)?;
    let lines = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let Some(line) = lines.first().copied() else {
        return Ok(None);
    };
    if lines.len() != 1 {
        return Err(ContextPackBuildError::InvalidBudget);
    }
    let value = line.trim();
    if value.is_empty() || line != value {
        return Err(ContextPackBuildError::InvalidBudget);
    }
    value
        .parse::<u64>()
        .map(Some)
        .map_err(|_error| ContextPackBuildError::InvalidBudget)
}

fn append_pinned_pack_candidates(
    context: &Path,
    candidates: &mut Vec<PackCandidate>,
) -> Result<(), ContextPackBuildError> {
    let pinned = context.join("pinned");
    let mut names = directory_entry_names(&pinned)?;
    names.sort();
    for name in names {
        if !is_safe_relative_file_name(&name) {
            continue;
        }
        let path = pinned.join(&name);
        if !path.is_file() {
            continue;
        }
        let source = format!("context/pinned/{name}");
        append_context_file_candidate(context, "system", &source, None, candidates)?;
    }
    Ok(())
}

fn append_context_file_candidate(
    context_dir: &Path,
    kind: &str,
    source: &str,
    range: Option<String>,
    candidates: &mut Vec<PackCandidate>,
) -> Result<(), ContextPackBuildError> {
    crate::context_pack_source::validate_context_pack_source(source)
        .map_err(|_error| ContextPackBuildError::CannotRead)?;
    let body = read_context_source(context_dir, source)?;
    if !body.trim().is_empty() {
        candidates.push(PackCandidate::new(kind, source, range, body));
    }
    Ok(())
}

fn append_context_jsonl_candidate(
    context_dir: &Path,
    kind: &str,
    source: &str,
    jsonl_kind: ContextJsonlKind,
    candidates: &mut Vec<PackCandidate>,
) -> Result<(), ContextPackBuildError> {
    crate::context_pack_source::validate_context_pack_source(source)
        .map_err(|_error| ContextPackBuildError::CannotRead)?;
    let body = read_context_source(context_dir, source)?;
    if body.trim().is_empty() {
        return Ok(());
    }
    if !inspect_context_jsonl(jsonl_kind, &body).is_ok() {
        return Err(ContextPackBuildError::InvalidContextJsonl);
    }
    candidates.push(PackCandidate::new(kind, source, None, body));
    Ok(())
}

fn append_recent_messages_candidate(
    messages: &str,
    recent_message_limit: usize,
    candidates: &mut Vec<PackCandidate>,
) {
    if recent_message_limit == 0 {
        return;
    }
    let lines = messages
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return;
    }
    let start = lines.len().saturating_sub(recent_message_limit);
    let selected = lines
        .get(start..)
        .map_or_else(String::new, |tail| tail.join("\n"));
    let range = format!("tail:{}", lines.len() - start);
    candidates.push(PackCandidate::new(
        "recent_messages",
        "messages.jsonl",
        Some(range),
        format!("{selected}\n"),
    ));
}

fn append_child_result_candidates(
    context: &Path,
    candidates: &mut Vec<PackCandidate>,
) -> Result<(), ContextPackBuildError> {
    let child_root = context.join("child");
    let mut names = directory_entry_names(&child_root)?;
    names.sort();
    for child in names {
        if !is_object_name(&child) {
            return Err(ContextPackBuildError::InvalidChildName);
        }
        let child_dir = child_root.join(&child);
        if !child_dir.is_dir() {
            continue;
        }
        let result_source = format!("context/child/{child}/result.md");
        append_context_file_candidate(context, "child_result", &result_source, None, candidates)?;

        let refs_source = format!("context/child/{child}/refs.jsonl");
        append_context_jsonl_candidate(
            context,
            "child_refs",
            &refs_source,
            ContextJsonlKind::Refs,
            candidates,
        )?;
    }
    Ok(())
}

fn read_context_source(context: &Path, source: &str) -> Result<String, ContextPackBuildError> {
    let relative = source
        .strip_prefix("context/")
        .ok_or(ContextPackBuildError::CannotRead)?;
    fs::read_to_string(context.join(relative)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ContextPackBuildError::MissingSession
        } else {
            ContextPackBuildError::CannotRead
        }
    })
}

fn select_pack_candidates(
    candidates: Vec<PackCandidate>,
    budget: Option<u64>,
) -> Vec<PackCandidate> {
    let Some(limit) = budget.filter(|limit| *limit > 0) else {
        return candidates;
    };
    let mut used = 0_u64;
    let mut selected = Vec::new();
    for candidate in candidates {
        if used.saturating_add(candidate.tokens) <= limit {
            used = used.saturating_add(candidate.tokens);
            selected.push(candidate);
        }
    }
    selected
}

fn render_context_pack(
    session: &str,
    agent: Option<&str>,
    budget: Option<u64>,
    candidates: &[PackCandidate],
) -> ContextPackBuild {
    let items = candidates
        .iter()
        .map(PackCandidate::item)
        .collect::<Vec<_>>();
    let json_items = items.iter().map(context_pack_item_json).collect::<Vec<_>>();
    let mut pack = serde_json::Map::new();
    pack.insert("session".to_owned(), serde_json::json!(session));
    pack.insert("items".to_owned(), serde_json::json!(json_items));
    if let Some(agent) = agent {
        pack.insert("agent".to_owned(), serde_json::json!(agent));
    }
    if let Some(budget) = budget {
        pack.insert("budget_tokens".to_owned(), serde_json::json!(budget));
    }

    let pack_json = format!("{}\n", Value::Object(pack));
    let pack_md = render_context_pack_markdown(session, agent, budget, candidates);
    ContextPackBuild::new(items, pack_json, pack_md)
}

fn context_pack_item_json(item: &ContextPackBuiltItem) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("kind".to_owned(), serde_json::json!(item.kind()));
    object.insert("source".to_owned(), serde_json::json!(item.source()));
    object.insert("tokens".to_owned(), serde_json::json!(item.tokens()));
    if let Some(range) = item.range() {
        object.insert("range".to_owned(), serde_json::json!(range));
    }
    Value::Object(object)
}

fn render_context_pack_markdown(
    session: &str,
    agent: Option<&str>,
    budget: Option<u64>,
    candidates: &[PackCandidate],
) -> String {
    let mut output = String::new();
    output.push_str("# CortexFS Context Pack\n\n");
    push_markdown_kv(&mut output, "session", session);
    if let Some(agent) = agent {
        push_markdown_kv(&mut output, "agent", agent);
    }
    if let Some(budget) = budget {
        push_markdown_kv(&mut output, "budget_tokens", &budget.to_string());
    }
    output.push('\n');

    for candidate in candidates {
        output.push_str("## ");
        output.push_str(&candidate.kind);
        output.push_str("\n\n");
        push_markdown_kv(&mut output, "source", &candidate.source);
        if let Some(range) = candidate.range.as_deref() {
            push_markdown_kv(&mut output, "range", range);
        }
        push_markdown_kv(&mut output, "tokens", &candidate.tokens.to_string());
        output.push('\n');
        output.push_str("```text\n");
        output.push_str(&candidate.content);
        if !candidate.content.ends_with('\n') {
            output.push('\n');
        }
        output.push_str("```\n\n");
    }

    output
}

fn push_markdown_kv(output: &mut String, key: &str, value: &str) {
    output.push_str(key);
    output.push_str(": ");
    output.push_str(value);
    output.push('\n');
}

fn directory_entry_names(path: &Path) -> Result<Vec<String>, ContextPackBuildError> {
    let entries = fs::read_dir(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ContextPackBuildError::MissingSession
        } else {
            ContextPackBuildError::CannotRead
        }
    })?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_error| ContextPackBuildError::CannotRead)?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or(ContextPackBuildError::CannotRead)?
            .to_owned();
        names.push(name);
    }
    Ok(names)
}

fn is_safe_relative_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\0')
        && !name.contains('\t')
        && !name.contains('\n')
        && name != "."
        && name != ".."
}

fn estimate_context_tokens(content: &str) -> u64 {
    let words = content.split_whitespace().count();
    u64::try_from(words.max(1)).unwrap_or(u64::MAX)
}
