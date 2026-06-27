use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path};

use serde::Deserialize;
use serde_json::Value;

use crate::{
    CHILD_RESULT_REQUIRED_DIRS, CHILD_RESULT_REQUIRED_FILES, CONTEXT_REQUIRED_DIRS,
    CONTEXT_REQUIRED_FILES, JsonStringField, SESSION_REQUIRED_FILES, abi_path,
    is_stable_chroot_absolute_path,
};

const MAX_SESSION_LAYOUT_CONTROL_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionLayoutIssue {
    MissingFile(String),
    MissingDirectory(String),
    NotFile(String),
    NotDirectory(String),
    InvalidFileValue { path: String, value: String },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionControlKind {
    State,
    Cwd,
    MetaJson,
}

impl SessionControlKind {
    /// Parses a durable session control file name.
    #[must_use]
    pub fn parse(file_name: &str) -> Option<Self> {
        match file_name {
            "state" => Some(Self::State),
            "cwd" => Some(Self::Cwd),
            "meta.json" => Some(Self::MetaJson),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionControlIssue {
    EmptyValue,
    MultipleValues { line: usize },
    InvalidValue { line: usize, value: String },
    InvalidJson,
    NotObject,
}

/// Result of inspecting a fixed-format session control file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionControlReport {
    issues: Vec<SessionControlIssue>,
}

impl SessionLayoutIssue {
    /// Returns a stable short description of the issue kind.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match *self {
            Self::MissingFile(_) => "missing file",
            Self::MissingDirectory(_) => "missing directory",
            Self::NotFile(_) => "not file",
            Self::NotDirectory(_) => "not directory",
            Self::InvalidFileValue { .. } => "invalid file value",
        }
    }

    /// Returns the relative session path associated with the issue.
    #[must_use]
    pub fn path(&self) -> &str {
        match *self {
            Self::MissingFile(ref path)
            | Self::MissingDirectory(ref path)
            | Self::NotFile(ref path)
            | Self::NotDirectory(ref path)
            | Self::InvalidFileValue { ref path, .. } => path,
        }
    }

    /// Returns the invalid value, when the issue records one.
    #[must_use]
    pub fn value(&self) -> Option<&str> {
        match *self {
            Self::InvalidFileValue { ref value, .. } => Some(value),
            Self::MissingFile(_)
            | Self::MissingDirectory(_)
            | Self::NotFile(_)
            | Self::NotDirectory(_) => None,
        }
    }
}

/// Result of inspecting a durable session directory.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionLayoutReport {
    issues: Vec<SessionLayoutIssue>,
}

impl_issue_report!(SessionLayoutReport, SessionLayoutIssue);
impl_issue_report!(SessionControlReport, SessionControlIssue);

/// Inspects a durable session directory for the v1 transparency/context layout.
#[must_use]
pub fn inspect_session_layout(session_dir: &Path) -> SessionLayoutReport {
    let mut issues = Vec::new();
    require_directory(session_dir, ".", &mut issues);
    for file in SESSION_REQUIRED_FILES {
        require_file(&session_dir.join(file), file, &mut issues);
    }
    inspect_session_control_files(session_dir, &mut issues);

    let context = session_dir.join("context");
    require_directory(&context, "context", &mut issues);
    for file in CONTEXT_REQUIRED_FILES {
        let label = format!("context/{file}");
        require_file(&context.join(file), &label, &mut issues);
    }
    for dir in CONTEXT_REQUIRED_DIRS {
        let label = format!("context/{dir}");
        require_directory(&context.join(dir), &label, &mut issues);
    }
    inspect_child_result_dirs(&context.join("child"), &mut issues);

    SessionLayoutReport::new(issues)
}

fn inspect_session_control_files(session_dir: &Path, issues: &mut Vec<SessionLayoutIssue>) {
    for file in SESSION_REQUIRED_FILES {
        let Some(kind) = SessionControlKind::parse(file) else {
            continue;
        };
        let Ok(content) = read_session_layout_control_file(&session_dir.join(file)) else {
            continue;
        };
        for issue in inspect_session_control(kind, &content).issues() {
            issues.push(SessionLayoutIssue::InvalidFileValue {
                path: (*file).to_owned(),
                value: session_control_issue_value(issue).to_owned(),
            });
        }
    }
}

fn session_control_issue_value(issue: &SessionControlIssue) -> &str {
    match *issue {
        SessionControlIssue::InvalidValue { ref value, .. } => value,
        SessionControlIssue::EmptyValue
        | SessionControlIssue::MultipleValues { .. }
        | SessionControlIssue::InvalidJson
        | SessionControlIssue::NotObject => "",
    }
}

/// Inspects a fixed-format v1 durable session control file body.
#[must_use]
pub fn inspect_session_control(kind: SessionControlKind, content: &str) -> SessionControlReport {
    match kind {
        SessionControlKind::State => inspect_session_state_control(content),
        SessionControlKind::Cwd => inspect_session_cwd_control(content),
        SessionControlKind::MetaJson => inspect_session_meta_json(content),
    }
}

fn inspect_session_state_control(content: &str) -> SessionControlReport {
    inspect_single_session_control_value(content, |line, value, issues| {
        if !matches!(value, "active" | "idle" | "done" | "error" | "cancelled") {
            issues.push(SessionControlIssue::InvalidValue {
                line,
                value: value.to_owned(),
            });
        }
    })
}

fn inspect_session_cwd_control(content: &str) -> SessionControlReport {
    inspect_single_session_control_value(content, |line, value, issues| {
        if !is_stable_chroot_absolute_path(value) {
            issues.push(SessionControlIssue::InvalidValue {
                line,
                value: value.to_owned(),
            });
        }
    })
}

fn inspect_single_session_control_value(
    content: &str,
    validate: impl Fn(usize, &str, &mut Vec<SessionControlIssue>),
) -> SessionControlReport {
    let mut issues = Vec::new();
    let mut lines = content.lines();
    let line = lines.next().unwrap_or("");
    let value = line.trim();
    if value.is_empty() {
        issues.push(SessionControlIssue::EmptyValue);
    } else if line != value {
        issues.push(SessionControlIssue::InvalidValue {
            line: 1,
            value: value.to_owned(),
        });
    } else {
        validate(1, value, &mut issues);
    }
    if lines.next().is_some() {
        issues.push(SessionControlIssue::MultipleValues { line: 2 });
    }
    SessionControlReport::new(issues)
}

fn inspect_session_meta_json(content: &str) -> SessionControlReport {
    if !content.trim_start().starts_with('{') {
        if serde_json::from_str::<Value>(content).is_ok() {
            return SessionControlReport::new(vec![SessionControlIssue::NotObject]);
        }
        return SessionControlReport::new(vec![SessionControlIssue::InvalidJson]);
    }
    let Ok(value) = serde_json::from_str::<Value>(content) else {
        return SessionControlReport::new(vec![SessionControlIssue::InvalidJson]);
    };
    if !value.is_object() {
        return SessionControlReport::new(vec![SessionControlIssue::NotObject]);
    }
    let Ok(meta) = serde_json::from_value::<SessionMetaJson>(value) else {
        return SessionControlReport::new(vec![SessionControlIssue::InvalidJson]);
    };

    let mut issues = Vec::new();
    inspect_optional_meta_string(meta.client.as_ref(), "client", &mut issues, |_| true);
    inspect_optional_meta_string(
        meta.model.as_ref(),
        "model",
        &mut issues,
        abi_path::is_model_reference,
    );
    inspect_optional_meta_string(meta.scope.as_ref(), "scope", &mut issues, |scope| {
        matches!(scope, "private" | "shared" | "temp")
    });
    SessionControlReport::new(issues)
}

#[derive(Deserialize)]
struct SessionMetaJson {
    client: Option<JsonStringField>,
    model: Option<JsonStringField>,
    scope: Option<JsonStringField>,
}

fn inspect_optional_meta_string(
    value: Option<&JsonStringField>,
    field: &str,
    issues: &mut Vec<SessionControlIssue>,
    valid: impl Fn(&str) -> bool,
) {
    let Some(value) = value else {
        return;
    };
    let Some(text) = value.as_str() else {
        issues.push(SessionControlIssue::InvalidValue {
            line: 1,
            value: field.to_owned(),
        });
        return;
    };
    if !valid(text) {
        issues.push(SessionControlIssue::InvalidValue {
            line: 1,
            value: text.to_owned(),
        });
    }
}

fn inspect_child_result_dirs(child_root: &Path, issues: &mut Vec<SessionLayoutIssue>) {
    let Ok(child_root_dir) = open_session_layout_plain_directory(child_root) else {
        return;
    };
    let Ok(entries) = fs::read_dir(session_layout_proc_fd_path(&child_root_dir)) else {
        return;
    };

    for entry in entries.flatten() {
        let child_name = entry.file_name().to_string_lossy().into_owned();
        let Ok(stat) = nix::sys::stat::fstatat(
            &child_root_dir,
            child_name.as_str(),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        ) else {
            continue;
        };
        if stat.st_mode & nix::libc::S_IFMT != nix::libc::S_IFDIR {
            continue;
        }
        let path = child_root.join(&child_name);
        inspect_child_result_dir(&path, &child_name, issues);
    }
}

fn inspect_child_result_dir(
    child_dir: &Path,
    child_name: &str,
    issues: &mut Vec<SessionLayoutIssue>,
) {
    for file in CHILD_RESULT_REQUIRED_FILES {
        let label = format!("context/child/{child_name}/{file}");
        require_file(&child_dir.join(file), &label, issues);
    }
    for dir in CHILD_RESULT_REQUIRED_DIRS {
        let label = format!("context/child/{child_name}/{dir}");
        require_directory(&child_dir.join(dir), &label, issues);
    }
}

fn require_file(path: &Path, label: &str, issues: &mut Vec<SessionLayoutIssue>) {
    match plain_path_metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_metadata) => issues.push(SessionLayoutIssue::NotFile(label.to_owned())),
        Err(_error) => issues.push(SessionLayoutIssue::MissingFile(label.to_owned())),
    }
}

fn require_directory(path: &Path, label: &str, issues: &mut Vec<SessionLayoutIssue>) {
    match plain_path_metadata(path) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_metadata) => issues.push(SessionLayoutIssue::NotDirectory(label.to_owned())),
        Err(_error) => issues.push(SessionLayoutIssue::MissingDirectory(label.to_owned())),
    }
}

fn read_session_layout_control_file(path: &Path) -> std::io::Result<String> {
    let mut file = open_session_layout_plain_file(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_SESSION_LAYOUT_CONTROL_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "session layout control file is too large or not a plain file",
        ));
    }
    let len = usize::try_from(metadata.len())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.utf8_error()))
}

fn plain_path_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_session_layout_plain_directory(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid file name")
        })?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_PATH | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    File::from(file_fd).metadata()
}

fn open_session_layout_plain_file(path: &Path) -> std::io::Result<File> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_session_layout_plain_directory(parent)?;
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

fn open_session_layout_plain_directory(path: &Path) -> std::io::Result<File> {
    let mut directory = if path.is_absolute() {
        open_session_layout_single_plain_directory(Path::new("/"))?
    } else {
        open_session_layout_single_plain_directory(Path::new("."))?
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

fn open_session_layout_single_plain_directory(path: &Path) -> std::io::Result<File> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ));
    }
    Ok(directory)
}

fn session_layout_proc_fd_path(directory: &File) -> std::path::PathBuf {
    std::path::PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()))
}
