use super::install::{
    InstallError, InstallTier, create_stage, install_class_path, rename_noreplace,
};
use super::receipt::{EntryKind, EntryReceipt, InspectedObject, entry_matches, inspect_object};
use super::residue::cleanup_residue;
use crate::ObjectClass;
use crate::support::plain::{open_plain_directory, proc_fd_path};

use std::ffi::OsString;
use std::fs;
#[cfg(test)]
use std::io::Write;
use std::path::{Path, PathBuf};

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

fn relative_stage(root: &Path, class: &Path, stage: &str) -> Result<PathBuf, InstallError> {
    class
        .join(stage)
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|_error| InstallError::unavailable("cannot derive uninstall residue path"))
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
    require_entry(class, name, executable, EntryKind::Executable)?;
    require_entry(class, &control_name, control, EntryKind::Directory)?;

    move_exact(
        class,
        name,
        stage,
        "executable",
        executable,
        EntryKind::Executable,
    )?;
    if let Err(detail) = sync_dirs(class, stage) {
        return Err(rollback_moves(
            class,
            stage,
            name,
            &control_name,
            executable,
            control,
            true,
            false,
            &detail,
        ));
    }
    if fault == 1 {
        #[cfg(test)]
        create_foreign_executable(class, name)?;
    }
    if let Err(detail) = require_missing(class, name) {
        return Err(rollback_moves(
            class,
            stage,
            name,
            &control_name,
            executable,
            control,
            true,
            false,
            &detail,
        ));
    }

    if let Err(detail) = move_exact(
        class,
        &control_name,
        stage,
        "control",
        control,
        EntryKind::Directory,
    ) {
        return Err(rollback_moves(
            class,
            stage,
            name,
            &control_name,
            executable,
            control,
            true,
            false,
            &detail,
        ));
    }
    if let Err(detail) = sync_dirs(class, stage) {
        return Err(rollback_moves(
            class,
            stage,
            name,
            &control_name,
            executable,
            control,
            true,
            true,
            &detail,
        ));
    }
    if fault == 2 {
        #[cfg(test)]
        create_foreign_control(class, &control_name)?;
    }
    let postcheck = require_missing(class, name)
        .and_then(|()| require_missing(class, &control_name))
        .and_then(|()| require_stage(stage, executable, control))
        .and_then(|()| sync_dirs(class, stage));
    if let Err(detail) = postcheck {
        return Err(rollback_moves(
            class,
            stage,
            name,
            &control_name,
            executable,
            control,
            true,
            true,
            &detail,
        ));
    }
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "rollback keeps both exact object receipts and path roles explicit"
)]
fn rollback_moves(
    class: &fs::File,
    stage: &fs::File,
    name: &str,
    control_name: &str,
    executable: EntryReceipt,
    control: EntryReceipt,
    executable_moved: bool,
    control_moved: bool,
    detail: &str,
) -> String {
    if control_moved
        && let Err(error) = restore_exact(
            stage,
            "control",
            class,
            control_name,
            control,
            EntryKind::Directory,
        )
    {
        return format!("{detail}; control rollback failed: {error}");
    }
    if executable_moved
        && let Err(error) = restore_exact(
            stage,
            "executable",
            class,
            name,
            executable,
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

fn move_exact(
    source: &fs::File,
    source_name: &str,
    target: &fs::File,
    target_name: &str,
    receipt: EntryReceipt,
    kind: EntryKind,
) -> Result<(), String> {
    require_entry(source, source_name, receipt, kind)?;
    rename_noreplace(source, source_name, target, target_name)
        .map_err(|error| format!("cannot quarantine object entry: {error}"))?;
    if entry_matches(target, target_name, receipt, kind)
        && require_missing(source, source_name).is_ok()
    {
        return Ok(());
    }
    let detail = "quarantined object entry did not match its retained receipt";
    match restore_exact(target, target_name, source, source_name, receipt, kind) {
        Ok(()) => Err(format!("{detail}; restored moved entry")),
        Err(error) => {
            let sync = sync_dirs(source, target)
                .err()
                .map_or_else(String::new, |sync| format!("; {sync}"));
            let disposition = if entry_matches(target, target_name, receipt, kind) {
                format!("; matching receipt retained as {target_name}")
            } else if entry_matches(source, source_name, receipt, kind) {
                format!("; matching receipt restored as {source_name}")
            } else {
                "; quarantine receipt no longer matches".to_owned()
            };
            Err(format!(
                "{detail}; restore failed: {error}{disposition}{sync}"
            ))
        }
    }
}

fn restore_exact(
    source: &fs::File,
    source_name: &str,
    target: &fs::File,
    target_name: &str,
    receipt: EntryReceipt,
    kind: EntryKind,
) -> Result<(), String> {
    require_entry(source, source_name, receipt, kind)?;
    require_missing(target, target_name)?;
    rename_noreplace(source, source_name, target, target_name)
        .map_err(|error| format!("cannot restore quarantined object entry: {error}"))?;
    require_entry(target, target_name, receipt, kind)?;
    require_missing(source, source_name)?;
    sync_dirs(source, target)
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

fn require_entry(
    parent: &fs::File,
    name: &str,
    receipt: EntryReceipt,
    kind: EntryKind,
) -> Result<(), String> {
    if entry_matches(parent, name, receipt, kind) {
        Ok(())
    } else {
        Err(format!(
            "object entry receipt changed: {name} expected dev={} ino={}",
            receipt.dev, receipt.ino
        ))
    }
}

fn require_missing(parent: &fs::File, name: &str) -> Result<(), String> {
    match nix::sys::stat::fstatat(parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
        Err(nix::errno::Errno::ENOENT) => Ok(()),
        Ok(stat) => Err(format!(
            "object path was recreated: {name} dev={} ino={}",
            stat.st_dev, stat.st_ino
        )),
        Err(error) => Err(format!(
            "cannot verify object path absence: {name}: {error}"
        )),
    }
}

fn sync_dirs(first: &fs::File, second: &fs::File) -> Result<(), String> {
    first
        .sync_all()
        .map_err(|error| format!("cannot sync object class: {error}"))?;
    second
        .sync_all()
        .map_err(|error| format!("cannot sync object quarantine: {error}"))
}

#[cfg(test)]
fn create_foreign_executable(class: &fs::File, name: &str) -> Result<(), String> {
    let fd = nix::fcntl::openat(
        class,
        name,
        nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_EXCL
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::from_bits_truncate(0o755),
    )
    .map_err(|error| format!("cannot create foreign executable: {error}"))?;
    let mut file = fs::File::from(fd);
    file.write_all(b"foreign")
        .map_err(|error| format!("cannot write foreign executable: {error}"))?;
    file.sync_all()
        .map_err(|error| format!("cannot sync foreign executable: {error}"))
}

#[cfg(test)]
fn create_foreign_control(class: &fs::File, name: &str) -> Result<(), String> {
    nix::sys::stat::mkdirat(class, name, nix::sys::stat::Mode::from_bits_truncate(0o700))
        .map_err(|error| format!("cannot create foreign control: {error}"))?;
    class
        .sync_all()
        .map_err(|error| format!("cannot sync foreign control: {error}"))
}

#[cfg(test)]
mod tests;
