use std::fs::{File, OpenOptions};
use std::io::Read as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path};

use nix::libc;

use crate::{atomic_replace_text, is_object_name};

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

impl_issue_report!(SessionIndexReport, SessionIndexIssue);

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
    let by_cwd_path = optional_index_path(
        &index_dir,
        "by-cwd",
        by_cwd_key,
        SessionIndexUpdateError::InvalidByCwdKey,
    )?;
    let by_hash_path = optional_index_path(
        &index_dir,
        "by-hash",
        by_hash_key,
        SessionIndexUpdateError::InvalidByHashKey,
    )?;
    let by_uuid_path = optional_index_path(
        &index_dir,
        "by-uuid",
        by_uuid_key,
        SessionIndexUpdateError::InvalidByUuidKey,
    )?;

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

    for path in [by_cwd_path, by_hash_path, by_uuid_path]
        .into_iter()
        .flatten()
    {
        atomic_replace_text(&path, &format!("{session_name}\n"))
            .map_err(|_error| SessionIndexUpdateError::CannotRecord)?;
    }

    Ok(())
}

fn optional_index_path(
    index_dir: &Path,
    dir_name: &str,
    key: Option<&str>,
    invalid_key: SessionIndexUpdateError,
) -> Result<Option<std::path::PathBuf>, SessionIndexUpdateError> {
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

fn is_plain_dir_path(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn is_plain_file_path(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_file())
}

fn read_session_index_file(path: &Path) -> std::io::Result<String> {
    let mut file = open_session_index_plain_file(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_SESSION_INDEX_FILE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "session index file is too large or not a plain file",
        ));
    }
    let len = usize::try_from(metadata.len())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.utf8_error()))
}

fn open_session_index_plain_file(path: &Path) -> std::io::Result<File> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_session_index_plain_directory(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid file name")
        })?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    Ok(File::from(file_fd))
}

fn open_session_index_plain_directory(path: &Path) -> std::io::Result<File> {
    let mut directory = if path.is_absolute() {
        open_session_index_single_plain_directory(Path::new("/"))?
    } else {
        open_session_index_single_plain_directory(Path::new("."))?
    };
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid directory name")
                })?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )
                .map_err(std::io::Error::from)?;
                directory = File::from(next);
            }
            Component::ParentDir | Component::Prefix(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_session_index_single_plain_directory(path: &Path) -> std::io::Result<File> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ));
    }
    Ok(directory)
}
