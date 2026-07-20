use crate::*;

use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::{
    CHILD_RESULT_REQUIRED_DIRS, CHILD_RESULT_REQUIRED_FILES, CONTEXT_REQUIRED_DIRS,
    CONTEXT_REQUIRED_FILES, ControlLineIssue, JsonStringField, PathLayoutIssue,
    SESSION_REQUIRED_FILES,
    abi::path::is_model_reference,
    is_stable_chroot_absolute_path,
    support::control::inspect_control_line,
    support::layout::{LayoutPathRole, require_plain},
    support::plain::{open_plain_directory, proc_fd_path, read_small_text_file},
};

const MAX_SESSION_LAYOUT_CONTROL_BYTES: u64 = 64 * 1024;

/// Session directory layout issues share the path-layout model.
pub type SessionLayoutIssue = PathLayoutIssue;

/// Session control-file issues share the control-line model.
pub type SessionControlIssue = ControlLineIssue;

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

/// Result of inspecting a fixed-format session control file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionControlReport {
    issues: Vec<ControlLineIssue>,
}

/// Result of inspecting a durable session directory.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionLayoutReport {
    issues: Vec<PathLayoutIssue>,
}

impl_issue_report!(SessionLayoutReport, PathLayoutIssue);
impl_issue_report!(SessionControlReport, ControlLineIssue);

/// Inspects a durable session directory for the stable transparency/context layout.
#[must_use]
pub fn inspect_session_layout(session_dir: &Path) -> SessionLayoutReport {
    let mut issues = Vec::new();
    require_plain(session_dir, ".", LayoutPathRole::Directory, &mut issues);
    for file in SESSION_REQUIRED_FILES {
        require_plain(
            &session_dir.join(file),
            file,
            LayoutPathRole::File,
            &mut issues,
        );
    }
    inspect_session_control_files(session_dir, &mut issues);

    let context = session_dir.join("context");
    require_plain(&context, "context", LayoutPathRole::Directory, &mut issues);
    for file in CONTEXT_REQUIRED_FILES {
        let label = format!("context/{file}");
        require_plain(
            &context.join(file),
            &label,
            LayoutPathRole::File,
            &mut issues,
        );
    }
    for dir in CONTEXT_REQUIRED_DIRS {
        let label = format!("context/{dir}");
        require_plain(
            &context.join(dir),
            &label,
            LayoutPathRole::Directory,
            &mut issues,
        );
    }
    inspect_child_result_dirs(&context.join("child"), &mut issues);

    SessionLayoutReport::new(issues)
}

pub(crate) fn inspect_session_control_files(session_dir: &Path, issues: &mut Vec<PathLayoutIssue>) {
    for file in SESSION_REQUIRED_FILES {
        let Some(kind) = SessionControlKind::parse(file) else {
            continue;
        };
        let Ok(content) = read_session_layout_control_file(&session_dir.join(file)) else {
            continue;
        };
        for issue in inspect_session_control(kind, &content).issues() {
            issues.push(PathLayoutIssue::invalid_value(
                *file,
                issue.value().unwrap_or(""),
            ));
        }
    }
}

/// Inspects a fixed-format durable session control file body.
#[must_use]
pub fn inspect_session_control(kind: SessionControlKind, content: &str) -> SessionControlReport {
    match kind {
        SessionControlKind::State => inspect_session_state_control(content),
        SessionControlKind::Cwd => inspect_session_cwd_control(content),
        SessionControlKind::MetaJson => inspect_session_meta_json(content),
    }
}

pub(crate) fn inspect_session_state_control(content: &str) -> SessionControlReport {
    SessionControlReport::new(inspect_control_line(
        content,
        true,
        |line, value, issues| {
            if !matches!(value, "active" | "idle" | "done" | "error" | "cancelled") {
                issues.push(ControlLineIssue::InvalidValue {
                    line,
                    value: value.to_owned(),
                });
            }
        },
    ))
}

pub(crate) fn inspect_session_cwd_control(content: &str) -> SessionControlReport {
    SessionControlReport::new(inspect_control_line(
        content,
        true,
        |line, value, issues| {
            if !is_stable_chroot_absolute_path(value) {
                issues.push(ControlLineIssue::InvalidValue {
                    line,
                    value: value.to_owned(),
                });
            }
        },
    ))
}

pub(crate) fn inspect_session_meta_json(content: &str) -> SessionControlReport {
    if !content.trim_start().starts_with('{') {
        if serde_json::from_str::<Value>(content).is_ok() {
            return SessionControlReport::new(vec![ControlLineIssue::NotObject]);
        }
        return SessionControlReport::new(vec![ControlLineIssue::InvalidJson]);
    }
    let Ok(value) = serde_json::from_str::<Value>(content) else {
        return SessionControlReport::new(vec![ControlLineIssue::InvalidJson]);
    };
    if !value.is_object() {
        return SessionControlReport::new(vec![ControlLineIssue::NotObject]);
    }
    let Ok(meta) = serde_json::from_str::<SessionMetaJson>(content) else {
        return SessionControlReport::new(vec![ControlLineIssue::InvalidJson]);
    };

    let mut issues = Vec::new();
    inspect_optional_meta_string(meta.client.as_ref(), "client", &mut issues, |_| true);
    inspect_optional_meta_string(
        meta.model.as_ref(),
        "model",
        &mut issues,
        is_model_reference,
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

pub(crate) fn inspect_optional_meta_string(
    value: Option<&JsonStringField>,
    field: &str,
    issues: &mut Vec<ControlLineIssue>,
    valid: impl Fn(&str) -> bool,
) {
    let Some(value) = value else {
        return;
    };
    let Some(text) = value.as_str() else {
        issues.push(ControlLineIssue::InvalidValue {
            line: 1,
            value: field.to_owned(),
        });
        return;
    };
    if !valid(text) {
        issues.push(ControlLineIssue::InvalidValue {
            line: 1,
            value: text.to_owned(),
        });
    }
}

pub(crate) fn inspect_child_result_dirs(child_root: &Path, issues: &mut Vec<PathLayoutIssue>) {
    let Ok(child_root_dir) = open_plain_directory(child_root) else {
        return;
    };
    let Ok(entries) = fs::read_dir(proc_fd_path(&child_root_dir)) else {
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
        if stat.st_mode & libc::S_IFMT != libc::S_IFDIR {
            continue;
        }
        let path = child_root.join(&child_name);
        inspect_child_result_dir(&path, &child_name, issues);
    }
}

pub(crate) fn inspect_child_result_dir(
    child_dir: &Path,
    child_name: &str,
    issues: &mut Vec<PathLayoutIssue>,
) {
    for file in CHILD_RESULT_REQUIRED_FILES {
        let label = format!("context/child/{child_name}/{file}");
        require_plain(&child_dir.join(file), &label, LayoutPathRole::File, issues);
    }
    for dir in CHILD_RESULT_REQUIRED_DIRS {
        let label = format!("context/child/{child_name}/{dir}");
        require_plain(
            &child_dir.join(dir),
            &label,
            LayoutPathRole::Directory,
            issues,
        );
    }
}

pub(crate) fn read_session_layout_control_file(path: &Path) -> std::io::Result<String> {
    read_small_text_file(path, MAX_SESSION_LAYOUT_CONTROL_BYTES)
}
