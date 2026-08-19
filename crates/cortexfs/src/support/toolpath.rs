use crate::*;

use std::collections::HashSet;
use std::fs::{self, File};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crate::{
    is_object_name,
    support::plain::{open_plain_directory, path_metadata_no_follow},
};
use nix::libc;

/// Error while resolving tool lookup through `CTX_PATH`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolPathError {
    /// Tool name is not a valid object name.
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

    /// Returns the default path: global tools first, then user tools.
    #[must_use]
    pub fn default(root: &Path, home: &Path) -> Self {
        Self::new([
            cortexfs_paths::tool_root_path(root),
            cortexfs_paths::home_tool_from_home_path(home),
        ])
    }

    /// Returns search directories in left-to-right lookup order.
    #[must_use]
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    /// Returns the path in the environment format used by executable tools.
    #[must_use]
    pub fn to_env(&self) -> String {
        self.dirs
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(":")
    }

    /// Returns whether this path only removes parent search tiers while
    /// preserving their first-hit order.
    #[must_use]
    pub(crate) fn is_ordered_subset_of(&self, parent: &Self) -> bool {
        let mut parent_offset = 0;
        let mut seen = HashSet::new();
        for dir in &self.dirs {
            if !seen.insert(dir) {
                return false;
            }
            let Some(offset) = parent
                .dirs
                .get(parent_offset..)
                .and_then(|dirs| dirs.iter().position(|parent_dir| parent_dir == dir))
            else {
                return false;
            };
            parent_offset += offset + 1;
        }
        true
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
        self.list_limited(usize::MAX, usize::MAX)
    }

    /// Lists at most `hit_limit` executable hits while inspecting at most
    /// `scan_limit` directory entries across all search tiers.
    pub fn list_limited(
        &self,
        hit_limit: usize,
        scan_limit: usize,
    ) -> Result<Vec<ToolHit>, ToolPathError> {
        let mut hits = Vec::new();
        let mut scanned = 0;
        for dir in &self.dirs {
            append_tool_hits(dir, &mut hits, hit_limit, scan_limit, &mut scanned)?;
            if hits.len() >= hit_limit || scanned >= scan_limit {
                break;
            }
        }
        Ok(hits)
    }
}

pub(crate) fn append_tool_hits(
    dir: &Path,
    hits: &mut Vec<ToolHit>,
    hit_limit: usize,
    scan_limit: usize,
    scanned: &mut usize,
) -> Result<(), ToolPathError> {
    let directory = match open_plain_directory(dir) {
        Ok(directory) => directory,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_error) => return Err(ToolPathError::CannotReadDirectory),
    };
    let entries = match fs::read_dir(support::plain::proc_fd_path(&directory)) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_error) => return Err(ToolPathError::CannotReadDirectory),
    };

    let mut local = Vec::new();
    for entry in entries {
        if *scanned >= scan_limit {
            break;
        }
        let entry = entry.map_err(|_error| ToolPathError::CannotReadDirectory)?;
        *scanned = scanned.saturating_add(1);
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_object_name(&name) && fd_entry_is_executable_file(&directory, &name) {
            local.push(ToolHit::new(dir.join(&name)));
        }
    }
    local.sort_by(|left, right| left.path.cmp(&right.path));
    local.truncate(hit_limit.saturating_sub(hits.len()));
    hits.extend(local);
    Ok(())
}

pub(crate) fn sibling_control_dir(path: &Path) -> PathBuf {
    let mut control = path.as_os_str().to_owned();
    control.push(".d");
    PathBuf::from(control)
}

/// Returns whether the path is an executable regular file.
#[must_use]
pub fn is_executable_file(path: &Path) -> bool {
    path_metadata_no_follow(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

pub(crate) fn fd_entry_is_executable_file(parent_dir: &File, name: &str) -> bool {
    nix::sys::stat::fstatat(parent_dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .is_ok_and(|stat| stat.st_mode & libc::S_IFMT == libc::S_IFREG && stat.st_mode & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    fn executable(path: &Path) -> io::Result<()> {
        fs::write(path, "#!/bin/sh\n")?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))
    }

    #[test]
    fn bounded_listing_separates_scan_and_hit_limits() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let first = root.path().join("first");
        let second = root.path().join("second");
        fs::create_dir_all(&first)?;
        fs::create_dir_all(&second)?;
        for index in 0..128 {
            fs::write(first.join(format!("junk-{index:03}")), "junk")?;
        }
        executable(&second.join("later"))?;
        let path = ToolPath::new([first, second]);
        assert!(
            path.list_limited(8, 64)
                .map_err(|error| { io::Error::other(format!("cannot list tools: {error:?}")) })?
                .is_empty()
        );

        let tier = root.path().join("tier");
        fs::create_dir_all(&tier)?;
        for name in ["zulu", "alpha", "middle"] {
            executable(&tier.join(name))?;
        }
        let hits = ToolPath::new([tier])
            .list_limited(2, 32)
            .map_err(|error| io::Error::other(format!("cannot list tools: {error:?}")))?;
        assert_eq!(
            hits.iter()
                .filter_map(|hit| hit.path().file_name()?.to_str())
                .collect::<Vec<_>>(),
            ["alpha", "middle"]
        );
        Ok(())
    }
}
