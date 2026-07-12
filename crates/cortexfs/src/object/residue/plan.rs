use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use super::{
    CLEANUP_PREFIX, INSTALL_PREFIX, MAX_RESIDUE_DEPTH, MAX_RESIDUE_ENTRIES, ROLLBACK_PREFIX,
    Receipt, ResidueError, ResidueFileKind, open_dir_at, read_names, receipt_at, require_receipt,
    verify_leaf_at,
};

pub(super) struct CleanupPlan {
    pub(super) top: fs::File,
    pub(super) entries: BTreeMap<PathBuf, Receipt>,
}

pub(super) fn validate_cleanup_path(path: &Path) -> Result<(PathBuf, String), ResidueError> {
    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(value) = component else {
            return Err(ResidueError::invalid(
                "residue cleanup path must be relative and contain no dot components",
            ));
        };
        let value = value
            .to_str()
            .ok_or_else(|| ResidueError::invalid("residue cleanup path must be valid UTF-8"))?;
        parts.push(value);
    }
    let Some(name) = parts.last().copied() else {
        return Err(ResidueError::invalid("residue cleanup path is empty"));
    };
    if name.as_bytes().starts_with(ROLLBACK_PREFIX) {
        return Err(ResidueError::invalid(
            "rollback residue is audit-only and cannot be cleaned by this command",
        ));
    }
    if name.as_bytes().starts_with(CLEANUP_PREFIX) {
        return Err(ResidueError::invalid(
            "cleanup quarantine is audit-only and cannot be cleaned by this command",
        ));
    }
    if !name.as_bytes().starts_with(INSTALL_PREFIX) || name.len() == INSTALL_PREFIX.len() {
        return Err(ResidueError::invalid(
            "residue cleanup accepts only .cortexfs-install-* stages",
        ));
    }
    let valid_parent = matches!(parts.as_slice(), ["tool" | "agent", _])
        || matches!(
            parts.as_slice(),
            ["home", uid, "tool" | "agent", _]
                if uid.bytes().all(|byte| byte.is_ascii_digit()) && uid.parse::<u32>().is_ok()
        );
    if !valid_parent {
        return Err(ResidueError::invalid(
            "install residue is outside a valid object install-class directory",
        ));
    }
    let mut parent = PathBuf::new();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        parent.push(part);
    }
    Ok((parent, name.to_owned()))
}

pub(super) fn is_install_path(path: &Path) -> bool {
    validate_cleanup_path(path).is_ok()
}

pub(super) fn open_relative_dir(
    root: &fs::File,
    path: &Path,
    root_dev: u64,
) -> Result<fs::File, ResidueError> {
    let mut directory = root.try_clone().map_err(|error| {
        ResidueError::unavailable(format!(
            "cannot duplicate durable source descriptor: {error}"
        ))
    })?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err(ResidueError::invalid("invalid residue parent path"));
        };
        let receipt = receipt_at(&directory, name).map_err(|error| {
            ResidueError::unavailable(format!("cannot inspect residue parent: {error}"))
        })?;
        if receipt.kind != ResidueFileKind::Directory || receipt.dev != root_dev {
            return Err(ResidueError::unavailable(
                "residue parent is not a same-device plain directory",
            ));
        }
        directory = open_dir_at(&directory, name, receipt)?;
    }
    Ok(directory)
}

pub(super) fn build_plan(
    parent: &fs::File,
    name: &str,
    expected: Receipt,
    root_dev: u64,
    requested: &Path,
) -> Result<CleanupPlan, ResidueError> {
    require_receipt(parent, OsStr::new(name), expected).map_err(|detail| {
        super::conflict(
            requested,
            expected.dev,
            expected.ino,
            None,
            "receipt",
            detail,
        )
    })?;
    if expected.dev != root_dev {
        return Err(ResidueError::invalid(
            "install residue receipt is on a different device from the durable source",
        ));
    }
    let top = open_dir_at(parent, OsStr::new(name), expected)?;
    let mut entries = BTreeMap::new();
    entries.insert(PathBuf::new(), expected);
    plan_dir(&top, Path::new(""), 0, root_dev, &mut entries)?;
    Ok(CleanupPlan { top, entries })
}

fn plan_dir(
    directory: &fs::File,
    relative: &Path,
    depth: usize,
    root_dev: u64,
    entries: &mut BTreeMap<PathBuf, Receipt>,
) -> Result<(), ResidueError> {
    if depth > MAX_RESIDUE_DEPTH {
        return Err(ResidueError::unavailable(
            "residue cleanup exceeded the directory depth limit",
        ));
    }
    let allowance = MAX_RESIDUE_ENTRIES.saturating_sub(entries.len());
    let names = read_names(directory, allowance)?;
    for name in names {
        if entries.len() >= MAX_RESIDUE_ENTRIES {
            return Err(ResidueError::unavailable(
                "residue cleanup exceeded the entry limit",
            ));
        }
        let receipt = receipt_at(directory, &name).map_err(|error| {
            ResidueError::unavailable(format!("cannot receipt cleanup entry: {error}"))
        })?;
        if receipt.dev != root_dev {
            return Err(ResidueError::unavailable(
                "residue cleanup refused a cross-device entry",
            ));
        }
        if !matches!(
            receipt.kind,
            ResidueFileKind::Directory | ResidueFileKind::File | ResidueFileKind::Symlink
        ) {
            return Err(ResidueError::unavailable(format!(
                "residue cleanup refused unknown file type at {}",
                relative.join(&name).display()
            )));
        }
        let path = relative.join(&name);
        if receipt.kind == ResidueFileKind::Directory {
            let child = open_dir_at(directory, &name, receipt)?;
            entries.insert(path.clone(), receipt);
            plan_dir(&child, &path, depth + 1, root_dev, entries)?;
        } else {
            verify_leaf_at(directory, &name, receipt)?;
            entries.insert(path, receipt);
        }
    }
    Ok(())
}

pub(super) fn open_plan_parent(
    top: &fs::File,
    path: &Path,
    entries: &BTreeMap<PathBuf, Receipt>,
) -> Result<fs::File, String> {
    let parent_path = path.parent().unwrap_or_else(|| Path::new(""));
    let mut directory = top
        .try_clone()
        .map_err(|error| format!("cannot duplicate isolated stage descriptor: {error}"))?;
    let mut current_path = PathBuf::new();
    for component in parent_path.components() {
        let Component::Normal(name) = component else {
            return Err("cleanup plan contains an invalid component".to_owned());
        };
        current_path.push(name);
        let expected = entries
            .get(&current_path)
            .copied()
            .ok_or_else(|| "cleanup plan is missing a parent receipt".to_owned())?;
        require_receipt(&directory, name, expected)?;
        directory = open_dir_at(&directory, name, expected).map_err(|error| error.to_string())?;
    }
    Ok(directory)
}
