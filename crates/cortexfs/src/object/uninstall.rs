use super::install::{InstallError, InstallTier, create_stage, install_class_path};
use super::receipt::{EntryKind, EntryReceipt, InspectedObject, inspect_object};
use super::residue::cleanup_residue;
use super::swap::{
    PairProgress, PairStep, quarantine_pair, relative_stage, require_entry, restore_exact,
    sync_dirs,
};
#[cfg(test)]
use super::swap::{create_foreign_control, create_foreign_executable};
use crate::ObjectClass;
use crate::support::plain::{open_plain_directory, proc_fd_path};

use std::ffi::OsString;
use std::fs;
use std::path::Path;

/// Inspects or removes one exact installer-managed object.
///
/// Dry-run inspection performs no writes. Applied removal first quarantines the
/// executable, then the control directory, and only deletes the exact stage
/// after both retained receipts and the complete stage shape are rechecked.
pub fn uninstall_object(
    root: &Path,
    class: ObjectClass,
    name: &str,
    tier: InstallTier,
    apply: bool,
) -> Result<InspectedObject, InstallError> {
    uninstall_with(root, class, name, tier, apply, 0)
}

fn uninstall_with(
    root: &Path,
    class: ObjectClass,
    name: &str,
    tier: InstallTier,
    apply: bool,
    fault: u8,
) -> Result<InspectedObject, InstallError> {
    let _source = open_plain_directory(root).map_err(|error| {
        InstallError::unavailable(format!("cannot open durable source: {error}"))
    })?;
    let inspected = inspect_object(root, class, name, tier)?;
    if !apply {
        return Ok(inspected);
    }

    let class_path = install_class_path(root, class, tier)?;
    let (stage_name, stage, stage_receipt) = create_stage(&inspected.class_fd)?;
    let stage_path = relative_stage(root, &class_path, &stage_name)?;
    let executable = EntryReceipt {
        dev: inspected.executable_dev(),
        ino: inspected.executable_ino(),
    };
    let control = EntryReceipt {
        dev: inspected.control_dev(),
        ino: inspected.control_ino(),
    };
    cleanup_residue(
        root,
        &stage_path,
        stage_receipt.dev,
        stage_receipt.ino,
        false,
    )
    .map_err(|error| {
        InstallError::unavailable(format!(
            "cannot prepare uninstall cleanup for {} dev={} ino={}: {error}",
            stage_path.display(),
            stage_receipt.dev,
            stage_receipt.ino,
        ))
    })?;
    if let Err(detail) = quarantine_object(&inspected, &stage, executable, control, fault) {
        return Err(InstallError::unavailable(format!(
            "object uninstall conflict: {detail}; retained residue path={} dev={} ino={}",
            stage_path.display(),
            stage_receipt.dev,
            stage_receipt.ino,
        )));
    }
    cleanup_residue(
        root,
        &stage_path,
        stage_receipt.dev,
        stage_receipt.ino,
        true,
    )
    .map_err(|error| {
        InstallError::unavailable(format!(
            "object was quarantined but uninstall cleanup failed for {}: {error}",
            stage_path.display()
        ))
    })?;
    Ok(inspected)
}

fn quarantine_object(
    inspected: &InspectedObject,
    stage: &fs::File,
    executable: EntryReceipt,
    control: EntryReceipt,
    fault: u8,
) -> Result<(), String> {
    let class = &inspected.class_fd;
    let name = inspected.name();
    let control_name = format!("{name}.d");
    let source = [name, control_name.as_str()];
    let target = ["executable", "control"];
    let receipt = [executable, control];
    let dirs = [class, stage];
    let mut progress = PairProgress::default();
    loop {
        progress = quarantine_pair(dirs, source, target, receipt, progress);
        if let Some(detail) = progress.detail.take() {
            if !progress.completed[0] {
                return Err(detail);
            }
            return Err(rollback_moves(
                class, stage, source, receipt, &progress, &detail,
            ));
        }
        match progress.phase {
            PairStep::ExecutableSynced if fault == 1 => {
                #[cfg(test)]
                create_foreign_executable(class, name)?;
            }
            PairStep::ControlSynced if fault == 2 => {
                #[cfg(test)]
                create_foreign_control(class, &control_name)?;
            }
            PairStep::Complete => break,
            _ => {}
        }
    }
    let postcheck =
        require_stage(stage, executable, control).and_then(|()| sync_dirs(class, stage));
    if let Err(detail) = postcheck {
        return Err(rollback_moves(
            class, stage, source, receipt, &progress, &detail,
        ));
    }
    Ok(())
}

fn rollback_moves(
    class: &fs::File,
    stage: &fs::File,
    source: [&str; 2],
    receipt: [EntryReceipt; 2],
    progress: &PairProgress,
    detail: &str,
) -> String {
    if progress.completed[1]
        && let Err(error) = restore_exact(
            stage,
            "control",
            class,
            source[1],
            receipt[1],
            EntryKind::Directory,
        )
    {
        return format!("{detail}; control rollback failed: {error}");
    }
    if progress.completed[0]
        && let Err(error) = restore_exact(
            stage,
            "executable",
            class,
            source[0],
            receipt[0],
            EntryKind::Executable,
        )
    {
        return format!("{detail}; executable rollback failed: {error}");
    }
    match sync_dirs(class, stage) {
        Ok(()) => format!("{detail}; restored installed object"),
        Err(error) => format!("{detail}; rollback sync failed: {error}"),
    }
}

fn require_stage(
    stage: &fs::File,
    executable: EntryReceipt,
    control: EntryReceipt,
) -> Result<(), String> {
    let mut names = Vec::new();
    let entries = fs::read_dir(proc_fd_path(stage))
        .map_err(|error| format!("cannot enumerate uninstall stage: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot enumerate uninstall stage: {error}"))?;
        if names.len() >= 3 {
            return Err("uninstall stage contains unexpected entries".to_owned());
        }
        names.push(entry.file_name());
    }
    names.sort();
    if names != [OsString::from("control"), OsString::from("executable")] {
        return Err("uninstall stage has an unexpected shape".to_owned());
    }
    require_entry(stage, "control", control, EntryKind::Directory)?;
    require_entry(stage, "executable", executable, EntryKind::Executable)
}

#[cfg(test)]
mod tests;
