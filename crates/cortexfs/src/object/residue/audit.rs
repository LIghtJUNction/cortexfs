use crate::support::plain::open_plain_directory;
use nix::libc;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use super::plan::is_install_path;
use super::{
    CLEANUP_PREFIX, INSTALL_PREFIX, MAX_RESIDUE_DEPTH, MAX_RESIDUE_ENTRIES, ROLLBACK_PREFIX,
    Receipt, ResidueEligibility, ResidueError, ResidueFileKind, ResidueKind, ResidueOccupancy,
    ResidueReport, open_dir_at, read_names, receipt_at,
};

struct AuditState {
    root_dev: u64,
    count: usize,
    reports: Vec<ResidueReport>,
}

/// Audits a durable source for install, cleanup, and rollback residue without following links.
///
/// The returned observations are sorted by relative path. They are not cleanup
/// authority; cleanup requires an explicit path plus the exact device and inode.
/// Unreadable, cross-device, or over-limit subtrees abort instead of being skipped.
pub fn audit_residue(source: &Path) -> Result<Vec<ResidueReport>, ResidueError> {
    let root = open_plain_directory(source).map_err(|error| {
        ResidueError::unavailable(format!("cannot open durable source: {error}"))
    })?;
    let metadata = root.metadata().map_err(|error| {
        ResidueError::unavailable(format!("cannot inspect durable source: {error}"))
    })?;
    if !metadata.is_dir() {
        return Err(ResidueError::invalid(
            "residue source must be a plain directory",
        ));
    }
    let names = read_names(&root, MAX_RESIDUE_ENTRIES)?;
    let mut state = AuditState {
        root_dev: metadata.dev(),
        count: 0,
        reports: Vec::new(),
    };
    audit_dir(&root, Path::new(""), names, 0, &mut state)?;
    state
        .reports
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(state.reports)
}

fn audit_dir(
    directory: &fs::File,
    relative: &Path,
    names: Vec<OsString>,
    depth: usize,
    state: &mut AuditState,
) -> Result<(), ResidueError> {
    if depth > MAX_RESIDUE_DEPTH {
        return Err(ResidueError::unavailable(
            "residue audit exceeded the directory depth limit",
        ));
    }
    for name in names {
        state.count = state.count.saturating_add(1);
        if state.count > MAX_RESIDUE_ENTRIES {
            return Err(ResidueError::unavailable(
                "residue audit exceeded the entry limit",
            ));
        }
        let receipt = match receipt_at(directory, &name) {
            Ok(receipt) => receipt,
            Err(error) if error.raw_os_error() == Some(libc::ENOENT) => continue,
            Err(error) => {
                return Err(ResidueError::unavailable(format!(
                    "cannot inspect durable residue entry: {error}"
                )));
            }
        };
        if receipt.dev != state.root_dev {
            return Err(ResidueError::unavailable(
                "residue audit refused a cross-device entry",
            ));
        }
        let path = relative.join(&name);
        let residue_kind = residue_kind(&name);
        if receipt.kind == ResidueFileKind::Directory {
            let child = open_dir_at(directory, &name, receipt)?;
            let allowance = MAX_RESIDUE_ENTRIES.saturating_sub(state.count);
            let child_names = read_names(&child, allowance)?;
            if let Some(kind) = residue_kind {
                state.reports.push(report(
                    kind,
                    path.clone(),
                    receipt,
                    if child_names.is_empty() {
                        ResidueOccupancy::Empty
                    } else {
                        ResidueOccupancy::Occupied
                    },
                ));
            }
            audit_dir(&child, &path, child_names, depth + 1, state)?;
        } else if let Some(kind) = residue_kind {
            state
                .reports
                .push(report(kind, path, receipt, ResidueOccupancy::Occupied));
        }
    }
    Ok(())
}

fn report(
    kind: ResidueKind,
    path: PathBuf,
    receipt: Receipt,
    occupancy: ResidueOccupancy,
) -> ResidueReport {
    let eligibility = if kind == ResidueKind::Install
        && receipt.kind == ResidueFileKind::Directory
        && is_install_path(&path)
    {
        ResidueEligibility::Eligible
    } else {
        ResidueEligibility::AuditOnly
    };
    ResidueReport {
        kind,
        path,
        dev: receipt.dev,
        ino: receipt.ino,
        file_kind: receipt.kind,
        occupancy,
        eligibility,
    }
}

fn residue_kind(name: &OsStr) -> Option<ResidueKind> {
    let bytes = name.as_bytes();
    if bytes.starts_with(INSTALL_PREFIX) && bytes.len() > INSTALL_PREFIX.len() {
        Some(ResidueKind::Install)
    } else if bytes.starts_with(CLEANUP_PREFIX) && bytes.len() > CLEANUP_PREFIX.len() {
        Some(ResidueKind::Cleanup)
    } else if bytes.starts_with(ROLLBACK_PREFIX) && bytes.len() > ROLLBACK_PREFIX.len() {
        Some(ResidueKind::Rollback)
    } else {
        None
    }
}
