use std::fs::File;
use std::io;

use nix::fcntl::{AtFlags, RenameFlags, renameat2};
use nix::sys::stat::fstatat;
use nix::unistd::{UnlinkatFlags, unlinkat};

use super::entry::{EntryKind, EntryReceipt, entry_matches, receipt_at};

pub fn park_entry(
    source: &File,
    source_name: &str,
    stage: &File,
    stage_name: &str,
    receipt: EntryReceipt,
    kind: EntryKind,
) -> io::Result<()> {
    require_entry(source, source_name, receipt, kind)?;
    renameat2(
        source,
        source_name,
        stage,
        stage_name,
        RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(io::Error::from)?;
    #[cfg(test)]
    super::hook::run(source, source_name)?;
    if entry_matches(stage, stage_name, receipt, kind) && missing(source, source_name)? {
        sync(source, stage)?;
        return Ok(());
    }
    if let Some(moved) = receipt_at(stage, stage_name, kind).ok().flatten() {
        let _restored = restore_entry(stage, stage_name, source, source_name, moved, kind);
    }
    Err(io::Error::other("parked entry receipt changed"))
}

pub fn restore_entry(
    source: &File,
    source_name: &str,
    target: &File,
    target_name: &str,
    receipt: EntryReceipt,
    kind: EntryKind,
) -> io::Result<()> {
    require_entry(source, source_name, receipt, kind)?;
    if !missing(target, target_name)? {
        return Err(io::Error::from(io::ErrorKind::AlreadyExists));
    }
    renameat2(
        source,
        source_name,
        target,
        target_name,
        RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(io::Error::from)?;
    if entry_matches(target, target_name, receipt, kind) && missing(source, source_name)? {
        return sync(source, target);
    }
    Err(io::Error::other("restored entry receipt changed"))
}

pub fn publish_entry(
    stage: &File,
    stage_name: &str,
    target: &File,
    target_name: &str,
    receipt: EntryReceipt,
    kind: EntryKind,
) -> io::Result<()> {
    restore_entry(stage, stage_name, target, target_name, receipt, kind)
}

pub fn remove_parked_entry(
    stage: &File,
    name: &str,
    receipt: EntryReceipt,
    kind: EntryKind,
) -> io::Result<()> {
    require_entry(stage, name, receipt, kind)?;
    let flags = if kind == EntryKind::Directory {
        UnlinkatFlags::RemoveDir
    } else {
        UnlinkatFlags::NoRemoveDir
    };
    unlinkat(stage, name, flags).map_err(io::Error::from)?;
    missing(stage, name)?
        .then_some(())
        .ok_or_else(|| io::Error::other("parked entry remains"))
}

fn require_entry(
    parent: &File,
    name: &str,
    receipt: EntryReceipt,
    kind: EntryKind,
) -> io::Result<()> {
    entry_matches(parent, name, receipt, kind)
        .then_some(())
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
}

fn missing(parent: &File, name: &str) -> io::Result<bool> {
    match fstatat(parent, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Err(nix::errno::Errno::ENOENT) => Ok(true),
        Ok(_) => Ok(false),
        Err(error) => Err(io::Error::from(error)),
    }
}

fn sync(first: &File, second: &File) -> io::Result<()> {
    first.sync_all()?;
    second.sync_all()
}
