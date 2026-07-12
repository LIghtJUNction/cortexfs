mod audit;
mod cleanup;
mod plan;
#[cfg(test)]
mod tests;

use crate::support::plain::proc_fd_path;
use nix::libc;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

pub use audit::audit_residue;
pub use cleanup::cleanup_residue;
#[cfg(test)]
use cleanup::{apply_cleanup, prepare_cleanup};

const INSTALL_PREFIX: &[u8] = b".cortexfs-install-";
const CLEANUP_PREFIX: &[u8] = b".cortexfs-cleanup-";
const ROLLBACK_PREFIX: &[u8] = b".ctx-rollback-";
const MAX_RESIDUE_DEPTH: usize = 32;
const MAX_RESIDUE_ENTRIES: usize = 16_384;

/// Origin of a residue observed in a durable source tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResidueKind {
    /// An interrupted object installation stage.
    Install,
    /// A cleanup quarantine that could not be restored to its install-stage name.
    Cleanup,
    /// A receipt-preserving rollback quarantine.
    Rollback,
}

impl ResidueKind {
    /// Returns the stable CLI word for this kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Cleanup => "cleanup",
            Self::Rollback => "rollback",
        }
    }
}

/// No-follow file kind recorded by a residue audit.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResidueFileKind {
    /// Directory inode.
    Directory,
    /// Regular file inode.
    File,
    /// Symbolic link inode.
    Symlink,
    /// Unix-domain socket inode.
    Socket,
    /// A file kind that cleanup does not understand.
    Other,
}

impl ResidueFileKind {
    /// Returns the stable CLI word for this file kind.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Directory => "directory",
            Self::File => "file",
            Self::Symlink => "symlink",
            Self::Socket => "socket",
            Self::Other => "other",
        }
    }

    const fn from_mode(mode: libc::mode_t) -> Self {
        match mode & libc::S_IFMT {
            libc::S_IFDIR => Self::Directory,
            libc::S_IFREG => Self::File,
            libc::S_IFLNK => Self::Symlink,
            libc::S_IFSOCK => Self::Socket,
            _ => Self::Other,
        }
    }
}

/// Whether an observed residue contains any immediate entries.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResidueOccupancy {
    /// The residue is an empty directory.
    Empty,
    /// The residue is not an empty directory.
    Occupied,
}

impl ResidueOccupancy {
    /// Returns the stable CLI word for this state.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Occupied => "occupied",
        }
    }
}

/// Whether an audit result may be submitted to explicit residue cleanup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResidueEligibility {
    /// The path is an install stage in a valid install-class directory.
    Eligible,
    /// The result is observation-only and this command will not remove it.
    AuditOnly,
}

impl ResidueEligibility {
    /// Returns the stable CLI word for this eligibility.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Eligible => "eligible",
            Self::AuditOnly => "audit-only",
        }
    }
}

/// One no-follow residue observation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResidueReport {
    /// Residue origin.
    pub kind: ResidueKind,
    /// Path relative to the durable source root.
    pub path: PathBuf,
    /// Device number observed without following the entry.
    pub dev: u64,
    /// Inode number observed without following the entry.
    pub ino: u64,
    /// Observed inode kind.
    pub file_kind: ResidueFileKind,
    /// Whether the entry is an empty directory.
    pub occupancy: ResidueOccupancy,
    /// Whether exact-receipt cleanup accepts the path.
    pub eligibility: ResidueEligibility,
}

/// Result of planning or applying exact-receipt cleanup.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResidueCleanupReport {
    /// Requested path relative to the durable source root.
    pub path: PathBuf,
    /// Submitted device receipt.
    pub dev: u64,
    /// Submitted inode receipt.
    pub ino: u64,
    /// Number of receipt-planned entries, including the top directory.
    pub entries: usize,
    /// Whether cleanup was applied instead of only planned.
    pub applied: bool,
}

/// Exact quarantine context retained after a cleanup conflict.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResidueConflict {
    /// Requested relative install-stage path.
    pub path: PathBuf,
    /// Submitted device receipt.
    pub dev: u64,
    /// Submitted inode receipt.
    pub ino: u64,
    /// Exact relative quarantine path retained after isolation, when known.
    pub quarantine: Option<PathBuf>,
    /// Stable cleanup phase name.
    pub stage: &'static str,
    /// Concrete failure detail.
    pub detail: String,
}

/// Failure while auditing or cleaning durable residue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResidueError {
    /// Invalid command input or cleanup target.
    Invalid(String),
    /// Durable source inspection could not be completed safely.
    Unavailable(String),
    /// A receipt changed or cleanup could not finish after isolation.
    Conflict(ResidueConflict),
}

impl ResidueError {
    fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid(message.into())
    }

    fn unavailable(message: impl Into<String>) -> Self {
        Self::Unavailable(message.into())
    }

    /// Returns whether this failure is invalid user input.
    #[must_use]
    pub const fn is_invalid(&self) -> bool {
        matches!(self, Self::Invalid(_))
    }
}

impl std::fmt::Display for ResidueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match *self {
            Self::Invalid(ref message) | Self::Unavailable(ref message) => f.write_str(message),
            Self::Conflict(ref conflict) => {
                write!(
                    f,
                    "residue cleanup conflict stage={} path={} dev={} ino={}",
                    conflict.stage,
                    conflict.path.display(),
                    conflict.dev,
                    conflict.ino
                )?;
                if let Some(ref quarantine) = conflict.quarantine {
                    write!(f, " quarantine={}", quarantine.display())?;
                }
                write!(f, ": {}", conflict.detail)
            }
        }
    }
}

impl std::error::Error for ResidueError {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Receipt {
    dev: u64,
    ino: u64,
    kind: ResidueFileKind,
}

fn read_names(directory: &fs::File, allowance: usize) -> Result<Vec<OsString>, ResidueError> {
    let mut names = Vec::new();
    let entries = fs::read_dir(proc_fd_path(directory)).map_err(|error| {
        ResidueError::unavailable(format!(
            "cannot enumerate durable residue directory: {error}"
        ))
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            ResidueError::unavailable(format!("cannot enumerate durable residue entry: {error}"))
        })?;
        if names.len() >= allowance {
            return Err(ResidueError::unavailable(
                "residue traversal exceeded the entry limit",
            ));
        }
        names.push(entry.file_name());
    }
    names.sort();
    Ok(names)
}

fn receipt_at(parent: &fs::File, name: &OsStr) -> io::Result<Receipt> {
    let stat = nix::sys::stat::fstatat(parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .map_err(io::Error::from)?;
    Ok(Receipt {
        dev: stat.st_dev,
        ino: stat.st_ino,
        kind: ResidueFileKind::from_mode(stat.st_mode),
    })
}

fn open_dir_at(
    parent: &fs::File,
    name: &OsStr,
    expected: Receipt,
) -> Result<fs::File, ResidueError> {
    let fd = nix::fcntl::openat(
        parent,
        name,
        nix::fcntl::OFlag::O_DIRECTORY
            | nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|error| {
        ResidueError::unavailable(format!("cannot open residue directory: {error}"))
    })?;
    let directory = fs::File::from(fd);
    let metadata = directory.metadata().map_err(|error| {
        ResidueError::unavailable(format!("cannot inspect residue directory: {error}"))
    })?;
    let actual = Receipt {
        dev: metadata.dev(),
        ino: metadata.ino(),
        kind: ResidueFileKind::from_mode(metadata.mode()),
    };
    if actual != expected || actual.kind != ResidueFileKind::Directory {
        return Err(ResidueError::unavailable(
            "residue directory receipt changed while opening",
        ));
    }
    require_receipt(parent, name, expected).map_err(ResidueError::unavailable)?;
    Ok(directory)
}

fn verify_leaf_at(parent: &fs::File, name: &OsStr, expected: Receipt) -> Result<(), ResidueError> {
    let fd = nix::fcntl::openat(
        parent,
        name,
        nix::fcntl::OFlag::O_PATH | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|error| ResidueError::unavailable(format!("cannot open cleanup receipt: {error}")))?;
    let file = fs::File::from(fd);
    let metadata = file.metadata().map_err(|error| {
        ResidueError::unavailable(format!("cannot inspect cleanup receipt: {error}"))
    })?;
    let actual = Receipt {
        dev: metadata.dev(),
        ino: metadata.ino(),
        kind: ResidueFileKind::from_mode(metadata.mode()),
    };
    if actual != expected {
        return Err(ResidueError::unavailable(
            "cleanup receipt changed while opening",
        ));
    }
    require_receipt(parent, name, expected).map_err(ResidueError::unavailable)
}

fn require_receipt(parent: &fs::File, name: &OsStr, expected: Receipt) -> Result<(), String> {
    let actual = receipt_at(parent, name)
        .map_err(|error| format!("cannot inspect exact cleanup receipt: {error}"))?;
    if actual != expected {
        return Err(format!(
            "receipt changed: expected dev={} ino={} type={}, got dev={} ino={} type={}",
            expected.dev,
            expected.ino,
            expected.kind.as_str(),
            actual.dev,
            actual.ino,
            actual.kind.as_str()
        ));
    }
    Ok(())
}

fn conflict(
    path: &Path,
    dev: u64,
    ino: u64,
    quarantine: Option<PathBuf>,
    stage: &'static str,
    detail: impl Into<String>,
) -> ResidueError {
    ResidueError::Conflict(ResidueConflict {
        path: path.to_path_buf(),
        dev,
        ino,
        quarantine,
        stage,
        detail: detail.into(),
    })
}
