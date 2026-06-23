use std::fs;
use std::path::Path;

use crate::{atomic_replace_text, is_object_name};

/// Stable session index file kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionIndexKind {
    /// `session/index/list`: one session name per line.
    List,
    /// `session/index/current`: single current session name.
    Current,
    /// `session/index/by-cwd/<hash>`: single session name for a cwd hash.
    ByCwd,
}

/// Session index validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionIndexIssue {
    /// A required session name value is empty.
    EmptyValue { line: usize },
    /// A single-value index file contains more than one line.
    MultipleValues { line: usize },
    /// Session name does not use the stable object-name syntax.
    InvalidSessionName { line: usize, value: String },
}

/// Result of inspecting a session index file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionIndexReport {
    issues: Vec<SessionIndexIssue>,
}

/// Durable session index update error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionIndexUpdateError {
    /// Session name does not use the stable object-name syntax.
    InvalidSessionName,
    /// Optional `by-cwd` key does not use the stable object-name syntax.
    InvalidByCwdKey,
    /// The target durable session directory is missing.
    MissingSession,
    /// The reserved `session/index` directory or required files are missing.
    MissingIndex,
    /// Existing index files are malformed.
    InvalidIndex,
    /// Index files could not be read or atomically rewritten.
    CannotRecord,
}

impl SessionIndexUpdateError {
    /// Returns a stable errno name for this update failure.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidSessionName | Self::InvalidByCwdKey | Self::InvalidIndex => "EINVAL",
            Self::MissingSession | Self::MissingIndex => "ENOENT",
            Self::CannotRecord => "EIO",
        }
    }
}

impl SessionIndexReport {
    /// Creates a report with collected session index issues.
    #[must_use]
    pub const fn new(issues: Vec<SessionIndexIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when the index file satisfies the fixed v1 format.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected session index issues.
    #[must_use]
    pub fn issues(&self) -> &[SessionIndexIssue] {
        &self.issues
    }
}

/// Inspects a fixed-format v1 session index file.
#[must_use]
pub fn inspect_session_index(kind: SessionIndexKind, content: &str) -> SessionIndexReport {
    match kind {
        SessionIndexKind::List => inspect_session_index_list(content),
        SessionIndexKind::Current | SessionIndexKind::ByCwd => {
            inspect_single_session_index_value(content)
        }
    }
}

fn inspect_session_index_list(content: &str) -> SessionIndexReport {
    let mut issues = Vec::new();
    for (index, raw_line) in content.lines().enumerate() {
        inspect_session_index_name(index + 1, raw_line, &mut issues);
    }
    SessionIndexReport::new(issues)
}

fn inspect_single_session_index_value(content: &str) -> SessionIndexReport {
    let mut issues = Vec::new();
    let mut lines = content.lines();
    if let Some(first) = lines.next() {
        inspect_session_index_name(1, first, &mut issues);
        if lines.next().is_some() {
            issues.push(SessionIndexIssue::MultipleValues { line: 2 });
        }
    } else {
        issues.push(SessionIndexIssue::EmptyValue { line: 1 });
    }
    SessionIndexReport::new(issues)
}

fn inspect_session_index_name(line: usize, raw_line: &str, issues: &mut Vec<SessionIndexIssue>) {
    let value = raw_line.trim();
    if value.is_empty() {
        issues.push(SessionIndexIssue::EmptyValue { line });
    } else if value != raw_line || !is_object_name(value) {
        issues.push(SessionIndexIssue::InvalidSessionName {
            line,
            value: value.to_owned(),
        });
    }
}

/// Updates the reserved durable session index files for a selected session.
///
/// This rewrites `index/current`, de-duplicates and prepends the session in
/// `index/list`, and optionally writes `index/by-cwd/<key>`. The caller owns
/// deriving a stable `by-cwd` key from a cwd; this function only preserves the
/// fixed index file formats.
pub fn update_session_index(
    session_root: &Path,
    session_name: &str,
    by_cwd_key: Option<&str>,
) -> Result<(), SessionIndexUpdateError> {
    if !is_object_name(session_name) {
        return Err(SessionIndexUpdateError::InvalidSessionName);
    }
    if !session_root.join(session_name).is_dir() {
        return Err(SessionIndexUpdateError::MissingSession);
    }
    let index_dir = session_root.join("index");
    let list_path = index_dir.join("list");
    let current_path = index_dir.join("current");
    if !index_dir.is_dir() || !list_path.is_file() || !current_path.is_file() {
        return Err(SessionIndexUpdateError::MissingIndex);
    }
    let by_cwd_path = if let Some(key) = by_cwd_key {
        if !is_object_name(key) {
            return Err(SessionIndexUpdateError::InvalidByCwdKey);
        }
        let by_cwd_dir = index_dir.join("by-cwd");
        if !by_cwd_dir.is_dir() {
            return Err(SessionIndexUpdateError::MissingIndex);
        }
        Some(by_cwd_dir.join(key))
    } else {
        None
    };

    let list =
        fs::read_to_string(&list_path).map_err(|_error| SessionIndexUpdateError::CannotRecord)?;
    if !inspect_session_index(SessionIndexKind::List, &list).is_ok() {
        return Err(SessionIndexUpdateError::InvalidIndex);
    }
    if !inspect_session_index(
        SessionIndexKind::Current,
        &fs::read_to_string(&current_path)
            .map_err(|_error| SessionIndexUpdateError::CannotRecord)?,
    )
    .is_ok()
    {
        return Err(SessionIndexUpdateError::InvalidIndex);
    }

    let mut sessions = vec![session_name.to_owned()];
    sessions.extend(
        list.lines()
            .filter(|existing| *existing != session_name)
            .map(str::to_owned),
    );
    atomic_replace_text(&list_path, &format!("{}\n", sessions.join("\n")))
        .map_err(|_error| SessionIndexUpdateError::CannotRecord)?;
    atomic_replace_text(&current_path, &format!("{session_name}\n"))
        .map_err(|_error| SessionIndexUpdateError::CannotRecord)?;

    if let Some(path) = by_cwd_path {
        atomic_replace_text(&path, &format!("{session_name}\n"))
            .map_err(|_error| SessionIndexUpdateError::CannotRecord)?;
    }

    Ok(())
}
