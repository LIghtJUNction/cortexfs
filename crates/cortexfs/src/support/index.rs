use std::path::{Path, PathBuf};

use crate::{
    ControlLineIssue, atomic_replace_text, is_object_name,
    support::control::{inspect_control_line, inspect_control_lines},
    support::plain::read_small_text_file,
};

const MAX_SESSION_INDEX_FILE_BYTES: u64 = 64 * 1024;

/// Stable session index file kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionIndexKind {
    /// `session/index/list`: one session name per line.
    List,
    /// `session/index/current`: single current session name.
    Current,
    /// `session/index/by-cwd/<hash>`: single session name for a cwd hash.
    ByCwd,
    /// `session/index/by-hash/<hash>`: single session name for an external hash.
    ByHash,
    /// `session/index/by-uuid/<uuid>`: single session name for an external uuid.
    ByUuid,
}

/// Session index validation uses the shared control-line issue model.
pub type SessionIndexIssue = ControlLineIssue;

/// Result of inspecting a session index file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionIndexReport {
    issues: Vec<ControlLineIssue>,
}

/// Durable session index update error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionIndexUpdateError {
    /// Session name does not use the stable object-name syntax.
    InvalidSessionName,
    /// Optional `by-cwd` key does not use the stable object-name syntax.
    InvalidByCwdKey,
    /// Optional `by-hash` key does not use the stable object-name syntax.
    InvalidByHashKey,
    /// Optional `by-uuid` key does not use the stable object-name syntax.
    InvalidByUuidKey,
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
            Self::InvalidSessionName
            | Self::InvalidByCwdKey
            | Self::InvalidByHashKey
            | Self::InvalidByUuidKey
            | Self::InvalidIndex => "EINVAL",
            Self::MissingSession | Self::MissingIndex => "ENOENT",
            Self::CannotRecord => "EIO",
        }
    }
}

impl_issue_report!(SessionIndexReport, ControlLineIssue);

/// Inspects a fixed-format v1 session index file.
#[must_use]
pub fn inspect_session_index(kind: SessionIndexKind, content: &str) -> SessionIndexReport {
    match kind {
        SessionIndexKind::List => inspect_session_index_list(content),
        SessionIndexKind::Current
        | SessionIndexKind::ByCwd
        | SessionIndexKind::ByHash
        | SessionIndexKind::ByUuid => inspect_single_session_index_value(content),
    }
}

pub(crate) fn inspect_session_index_list(content: &str) -> SessionIndexReport {
    SessionIndexReport::new(inspect_control_lines(content, |line, value, issues| {
        if !is_object_name(value) {
            issues.push(ControlLineIssue::InvalidValue {
                line,
                value: value.to_owned(),
            });
        }
    }))
}

pub(crate) fn inspect_single_session_index_value(content: &str) -> SessionIndexReport {
    SessionIndexReport::new(inspect_control_line(
        content,
        true,
        |line, value, issues| {
            if !is_object_name(value) {
                issues.push(ControlLineIssue::InvalidValue {
                    line,
                    value: value.to_owned(),
                });
            }
        },
    ))
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
    update_session_index_with_keys(session_root, session_name, by_cwd_key, None, None)
}

/// Updates `index/list`, `index/current`, and optional secondary session indexes.
///
/// `by-hash` and `by-uuid` keys are caller-supplied because `CortexFS` does not
/// define one canonical source for those identities.
pub fn update_session_index_with_keys(
    session_root: &Path,
    session_name: &str,
    by_cwd_key: Option<&str>,
    by_hash_key: Option<&str>,
    by_uuid_key: Option<&str>,
) -> Result<(), SessionIndexUpdateError> {
    let update = prepare_session_index_update(
        session_root,
        session_name,
        by_cwd_key,
        by_hash_key,
        by_uuid_key,
    )?;

    let mut sessions = vec![session_name.to_owned()];
    sessions.extend(
        update
            .list
            .lines()
            .filter(|existing| *existing != session_name)
            .map(str::to_owned),
    );
    atomic_replace_text(&update.list_path, &format!("{}\n", sessions.join("\n")))
        .map_err(|_error| SessionIndexUpdateError::CannotRecord)?;
    atomic_replace_text(&update.current_path, &format!("{session_name}\n"))
        .map_err(|_error| SessionIndexUpdateError::CannotRecord)?;

    for path in update.secondary_paths.into_iter().flatten() {
        atomic_replace_text(&path, &format!("{session_name}\n"))
            .map_err(|_error| SessionIndexUpdateError::CannotRecord)?;
    }

    Ok(())
}

pub fn preflight_session_index_update(
    session_root: &Path,
    session_name: &str,
    by_cwd_key: Option<&str>,
    by_hash_key: Option<&str>,
    by_uuid_key: Option<&str>,
) -> Result<(), SessionIndexUpdateError> {
    prepare_session_index_update(
        session_root,
        session_name,
        by_cwd_key,
        by_hash_key,
        by_uuid_key,
    )
    .map(|_update| ())
}

pub(crate) struct SessionIndexUpdate {
    pub(crate) list_path: PathBuf,
    pub(crate) current_path: PathBuf,
    pub(crate) secondary_paths: [Option<PathBuf>; 3],
    pub(crate) list: String,
}

pub(crate) fn prepare_session_index_update(
    session_root: &Path,
    session_name: &str,
    by_cwd_key: Option<&str>,
    by_hash_key: Option<&str>,
    by_uuid_key: Option<&str>,
) -> Result<SessionIndexUpdate, SessionIndexUpdateError> {
    if !is_object_name(session_name) {
        return Err(SessionIndexUpdateError::InvalidSessionName);
    }
    if !is_plain_dir_path(&session_root.join(session_name)) {
        return Err(SessionIndexUpdateError::MissingSession);
    }
    let index_dir = session_root.join("index");
    let list_path = index_dir.join("list");
    let current_path = index_dir.join("current");
    if !is_plain_dir_path(&index_dir)
        || !is_plain_file_path(&list_path)
        || !is_plain_file_path(&current_path)
    {
        return Err(SessionIndexUpdateError::MissingIndex);
    }
    let secondary_paths = [
        optional_index_path(
            &index_dir,
            "by-cwd",
            by_cwd_key,
            SessionIndexUpdateError::InvalidByCwdKey,
        )?,
        optional_index_path(
            &index_dir,
            "by-hash",
            by_hash_key,
            SessionIndexUpdateError::InvalidByHashKey,
        )?,
        optional_index_path(
            &index_dir,
            "by-uuid",
            by_uuid_key,
            SessionIndexUpdateError::InvalidByUuidKey,
        )?,
    ];

    let list = read_session_index_file(&list_path)
        .map_err(|_error| SessionIndexUpdateError::CannotRecord)?;
    if !inspect_session_index(SessionIndexKind::List, &list).is_ok() {
        return Err(SessionIndexUpdateError::InvalidIndex);
    }
    if !inspect_session_index(
        SessionIndexKind::Current,
        &read_session_index_file(&current_path)
            .map_err(|_error| SessionIndexUpdateError::CannotRecord)?,
    )
    .is_ok()
    {
        return Err(SessionIndexUpdateError::InvalidIndex);
    }

    Ok(SessionIndexUpdate {
        list_path,
        current_path,
        secondary_paths,
        list,
    })
}

pub(crate) fn optional_index_path(
    index_dir: &Path,
    dir_name: &str,
    key: Option<&str>,
    invalid_key: SessionIndexUpdateError,
) -> Result<Option<PathBuf>, SessionIndexUpdateError> {
    let Some(key) = key else {
        return Ok(None);
    };
    if !is_object_name(key) {
        return Err(invalid_key);
    }
    let dir = index_dir.join(dir_name);
    if !is_plain_dir_path(&dir) {
        return Err(SessionIndexUpdateError::MissingIndex);
    }
    Ok(Some(dir.join(key)))
}

pub(crate) fn is_plain_dir_path(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_dir())
}

pub(crate) fn is_plain_file_path(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_file())
}

pub(crate) fn read_session_index_file(path: &Path) -> std::io::Result<String> {
    read_small_text_file(path, MAX_SESSION_INDEX_FILE_BYTES)
}
