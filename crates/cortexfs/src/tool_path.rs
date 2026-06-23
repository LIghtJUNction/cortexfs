use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::is_object_name;

/// Error while resolving tool lookup through `CTX_PATH`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolPathError {
    /// Tool name is not a valid v1 object name.
    InvalidName,
    /// Reading a lookup directory failed for a reason other than it not existing.
    CannotReadDirectory,
}

/// One executable tool found through `CTX_PATH`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolHit {
    path: PathBuf,
    control_dir: PathBuf,
}

impl ToolHit {
    /// Creates a tool lookup hit and derives the matching `.d/` control dir.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        let control_dir = sibling_control_dir(&path);
        Self { path, control_dir }
    }

    /// Returns the executable tool path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the matching `.d/` control directory for this exact executable.
    #[must_use]
    pub fn control_dir(&self) -> &Path {
        &self.control_dir
    }
}

/// Agent/tool search path for executable capability endpoints.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolPath {
    dirs: Vec<PathBuf>,
}

impl ToolPath {
    /// Builds a `CTX_PATH` from already split directories.
    #[must_use]
    pub fn new(dirs: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            dirs: dirs.into_iter().collect(),
        }
    }

    /// Parses the Unix `CTX_PATH` form. Empty components are ignored so the
    /// current working directory is never implicitly a tool directory.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        Self::new(
            value
                .split(':')
                .filter(|component| !component.is_empty())
                .map(PathBuf::from),
        )
    }

    /// Returns the v1 default path: global tools first, then user tools.
    #[must_use]
    pub fn default(root: &Path, home: &Path) -> Self {
        Self::new([root.join("tool"), home.join("tool")])
    }

    /// Returns search directories in left-to-right lookup order.
    #[must_use]
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    /// Finds the first executable file matching `name`.
    pub fn find(&self, name: &str) -> Result<Option<ToolHit>, ToolPathError> {
        if !is_object_name(name) {
            return Err(ToolPathError::InvalidName);
        }

        for dir in &self.dirs {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Ok(Some(ToolHit::new(candidate)));
            }
        }

        Ok(None)
    }

    /// Lists executable tool hits in lookup order. Non-executable files,
    /// sockets, and control directories are not hits.
    pub fn list(&self) -> Result<Vec<ToolHit>, ToolPathError> {
        let mut hits = Vec::new();
        for dir in &self.dirs {
            append_tool_hits(dir, &mut hits)?;
        }
        Ok(hits)
    }
}

fn append_tool_hits(dir: &Path, hits: &mut Vec<ToolHit>) -> Result<(), ToolPathError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_error) => return Err(ToolPathError::CannotReadDirectory),
    };

    let mut local = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_error| ToolPathError::CannotReadDirectory)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_object_name(&name) {
            let path = entry.path();
            if is_executable_file(&path) {
                local.push(ToolHit::new(path));
            }
        }
    }
    local.sort_by(|left, right| left.path.cmp(&right.path));
    hits.extend(local);
    Ok(())
}

fn sibling_control_dir(path: &Path) -> PathBuf {
    let mut control = path.as_os_str().to_owned();
    control.push(".d");
    PathBuf::from(control)
}

/// Returns whether the path is an executable regular file.
#[must_use]
pub fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}
