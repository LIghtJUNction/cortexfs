use super::install::{InstallError, rename_noreplace};
use super::receipt::{EntryKind, EntryReceipt, entry_matches};
#[cfg(test)]
use crate::support::plain::proc_fd_path;

#[cfg(test)]
use std::cell::Cell;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
thread_local! {
    static RECREATED_SOURCE: Cell<Option<&'static str>> = const { Cell::new(None) };
}

pub(super) fn relative_stage(
    root: &Path,
    class: &Path,
    stage: &str,
) -> Result<PathBuf, InstallError> {
    class
        .join(stage)
        .strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|_error| InstallError::unavailable("cannot derive object residue path"))
}

pub(super) fn move_exact(
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
    #[cfg(test)]
    recreate_source(source, source_name)?;
    verify_moved(source, source_name, target, target_name, receipt, kind)
}

#[cfg(test)]
pub(super) fn set_recreated_source(name: Option<&'static str>) -> Option<&'static str> {
    RECREATED_SOURCE.with(|fault| fault.replace(name))
}

#[cfg(test)]
fn recreate_source(source: &fs::File, source_name: &str) -> Result<(), String> {
    let enabled = RECREATED_SOURCE.with(|fault| {
        if fault.get() == Some(source_name) {
            fault.set(None);
            true
        } else {
            false
        }
    });
    if !enabled {
        return Ok(());
    }
    std::os::unix::net::UnixListener::bind(proc_fd_path(source).join(source_name))
        .map(|_listener| ())
        .map_err(|error| format!("cannot recreate moved stage source: {error}"))
}

fn verify_moved(
    source: &fs::File,
    source_name: &str,
    target: &fs::File,
    target_name: &str,
    receipt: EntryReceipt,
    kind: EntryKind,
) -> Result<(), String> {
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

pub(super) fn restore_exact(
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

pub(super) fn require_entry(
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

pub(super) fn require_missing(parent: &fs::File, name: &str) -> Result<(), String> {
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

pub(super) fn sync_dirs(first: &fs::File, second: &fs::File) -> Result<(), String> {
    first
        .sync_all()
        .map_err(|error| format!("cannot sync object class: {error}"))?;
    second
        .sync_all()
        .map_err(|error| format!("cannot sync object quarantine: {error}"))
}

#[cfg(test)]
pub(super) fn create_foreign_executable(parent: &fs::File, name: &str) -> Result<(), String> {
    use std::io::Write as _;

    let fd = nix::fcntl::openat(
        parent,
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
        .map_err(|error| format!("cannot sync foreign executable: {error}"))?;
    parent
        .sync_all()
        .map_err(|error| format!("cannot sync foreign executable parent: {error}"))
}

#[cfg(test)]
pub(super) fn create_foreign_control(parent: &fs::File, name: &str) -> Result<(), String> {
    nix::sys::stat::mkdirat(
        parent,
        name,
        nix::sys::stat::Mode::from_bits_truncate(0o700),
    )
    .map_err(|error| format!("cannot create foreign control: {error}"))?;
    parent
        .sync_all()
        .map_err(|error| format!("cannot sync foreign control: {error}"))
}
