//! Shared path-kind model for layout inspection issues.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use crate::plain_fs::path_metadata_no_follow;

/// Expected path role used when reporting missing / wrong-kind layout issues.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LayoutPathRole {
    /// Ordinary regular file.
    File,
    /// Directory.
    Directory,
    /// Executable regular file.
    Executable,
    /// Object control file.
    ControlFile,
    /// Object control directory.
    ControlDirectory,
    /// Unix domain socket.
    Socket,
}

impl LayoutPathRole {
    /// Label for a missing path of this role.
    #[must_use]
    pub const fn missing_label(self) -> &'static str {
        match self {
            Self::File => "missing file",
            Self::Directory => "missing directory",
            Self::Executable => "missing executable",
            Self::ControlFile => "missing control file",
            Self::ControlDirectory => "missing control directory",
            Self::Socket => "missing socket",
        }
    }

    /// Label for a wrong-kind path of this role.
    #[must_use]
    pub const fn wrong_label(self) -> &'static str {
        match self {
            Self::File => "not file",
            Self::Directory => "not directory",
            Self::Executable => "not executable",
            Self::ControlFile => "not control file",
            Self::ControlDirectory => "not control directory",
            Self::Socket => "not socket",
        }
    }
}

/// Shared layout path issue for object / session / shared-queue inspection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PathLayoutIssue {
    /// Path is missing.
    Missing { path: String, role: LayoutPathRole },
    /// Path exists but has the wrong kind.
    WrongKind { path: String, role: LayoutPathRole },
    /// Path content / control value is invalid.
    InvalidValue { path: String, value: String },
}

impl PathLayoutIssue {
    /// Short kind label used by formatters.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match *self {
            Self::Missing { role, .. } => role.missing_label(),
            Self::WrongKind { role, .. } => role.wrong_label(),
            Self::InvalidValue { .. } => "invalid value",
        }
    }

    /// Relative path associated with the issue.
    #[must_use]
    pub fn path(&self) -> &str {
        match *self {
            Self::Missing { ref path, .. }
            | Self::WrongKind { ref path, .. }
            | Self::InvalidValue { ref path, .. } => path,
        }
    }

    /// Invalid value when recorded.
    #[must_use]
    pub fn value(&self) -> Option<&str> {
        match *self {
            Self::InvalidValue { ref value, .. } => Some(value),
            Self::Missing { .. } | Self::WrongKind { .. } => None,
        }
    }

    #[must_use]
    pub fn missing(path: impl Into<String>, role: LayoutPathRole) -> Self {
        Self::Missing {
            path: path.into(),
            role,
        }
    }

    #[must_use]
    pub fn wrong_kind(path: impl Into<String>, role: LayoutPathRole) -> Self {
        Self::WrongKind {
            path: path.into(),
            role,
        }
    }

    #[must_use]
    pub fn invalid_value(path: impl Into<String>, value: impl Into<String>) -> Self {
        Self::InvalidValue {
            path: path.into(),
            value: value.into(),
        }
    }
}

/// Result of checking a plain (no-follow) path against an expected kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlainPathKindCheck {
    Ok,
    Missing,
    WrongKind,
}

/// Checks that `path` is a regular file without following the final symlink.
#[must_use]
pub fn check_plain_file(path: &Path) -> PlainPathKindCheck {
    match path_metadata_no_follow(path) {
        Ok(metadata) if metadata.is_file() => PlainPathKindCheck::Ok,
        Ok(_metadata) => PlainPathKindCheck::WrongKind,
        Err(_error) => PlainPathKindCheck::Missing,
    }
}

/// Checks that `path` is a directory without following the final symlink.
#[must_use]
pub fn check_plain_dir(path: &Path) -> PlainPathKindCheck {
    match path_metadata_no_follow(path) {
        Ok(metadata) if metadata.is_dir() => PlainPathKindCheck::Ok,
        Ok(_metadata) => PlainPathKindCheck::WrongKind,
        Err(_error) => PlainPathKindCheck::Missing,
    }
}

/// Checks that `path` is an executable regular file without following the final symlink.
#[must_use]
pub fn check_plain_executable(path: &Path) -> PlainPathKindCheck {
    match path_metadata_no_follow(path) {
        Ok(metadata) if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 => {
            PlainPathKindCheck::Ok
        }
        Ok(_metadata) => PlainPathKindCheck::WrongKind,
        Err(_error) => PlainPathKindCheck::Missing,
    }
}

/// Classifies a directory via `symlink_metadata`.
#[must_use]
pub fn check_symlink_dir(path: &Path) -> PlainPathKindCheck {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_dir() => PlainPathKindCheck::Ok,
        Ok(_metadata) => PlainPathKindCheck::WrongKind,
        Err(_error) => PlainPathKindCheck::Missing,
    }
}

/// Pushes a missing / wrong-kind issue for a plain path role.
pub fn push_plain_role_issues(
    check: PlainPathKindCheck,
    path_label: &str,
    role: LayoutPathRole,
    issues: &mut Vec<PathLayoutIssue>,
) {
    match check {
        PlainPathKindCheck::Ok => {}
        PlainPathKindCheck::Missing => {
            issues.push(PathLayoutIssue::missing(path_label, role));
        }
        PlainPathKindCheck::WrongKind => {
            issues.push(PathLayoutIssue::wrong_kind(path_label, role));
        }
    }
}

/// Single entry for plain (no-follow) layout role checks.
///
/// Prefer this over ad-hoc missing/wrong-kind pushes. For directories inspected
/// with `symlink_metadata` (shared queue), use [`require_symlink_dir`].
/// Socket paths use specialized callers (e.g. object layout), not this helper.
pub fn require_plain(
    path: &Path,
    label: &str,
    role: LayoutPathRole,
    issues: &mut Vec<PathLayoutIssue>,
) {
    let check = match role {
        LayoutPathRole::File | LayoutPathRole::ControlFile => check_plain_file(path),
        LayoutPathRole::Directory | LayoutPathRole::ControlDirectory => check_plain_dir(path),
        LayoutPathRole::Executable => check_plain_executable(path),
        LayoutPathRole::Socket => {
            // Socket inspection is not plain-path metadata; treat as wrong kind
            // only if something exists that is not handled by specialized code.
            match path_metadata_no_follow(path) {
                Ok(_metadata) => PlainPathKindCheck::WrongKind,
                Err(_error) => PlainPathKindCheck::Missing,
            }
        }
    };
    push_plain_role_issues(check, label, role, issues);
}

/// Directory check via `symlink_metadata` (shared-queue layout).
pub fn require_symlink_dir(path: &Path, label: &str, issues: &mut Vec<PathLayoutIssue>) {
    push_plain_role_issues(
        check_symlink_dir(path),
        label,
        LayoutPathRole::Directory,
        issues,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn path_layout_issue_labels_and_plain_checks() {
        let root =
            std::env::temp_dir().join(format!("cortexfs-layout-path-{}", std::process::id()));
        let _ignored = fs::remove_dir_all(&root);
        assert!(fs::create_dir_all(&root).is_ok());
        let file = root.join("file");
        let dir = root.join("dir");
        assert!(fs::write(&file, b"x").is_ok());
        assert!(fs::create_dir_all(&dir).is_ok());
        assert!(fs::set_permissions(&file, fs::Permissions::from_mode(0o644)).is_ok());

        assert_eq!(check_plain_file(&file), PlainPathKindCheck::Ok);
        assert_eq!(check_plain_dir(&dir), PlainPathKindCheck::Ok);
        assert_eq!(check_plain_executable(&file), PlainPathKindCheck::WrongKind);

        let mut issues = Vec::new();
        require_plain(&file, "file", LayoutPathRole::File, &mut issues);
        require_plain(&dir, "dir", LayoutPathRole::Directory, &mut issues);
        require_plain(&file, "exec", LayoutPathRole::Executable, &mut issues);
        assert!(
            issues
                .iter()
                .any(|issue| { issue.path() == "exec" && issue.kind() == "not executable" })
        );

        let issue = PathLayoutIssue::missing("agent/x", LayoutPathRole::Executable);
        assert_eq!(issue.kind(), "missing executable");
        assert_eq!(issue.path(), "agent/x");

        let _ignored = fs::remove_dir_all(&root);
    }
}
