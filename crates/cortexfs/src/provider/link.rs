use std::fs::File;
use std::io;
use std::path::Path;

use crate::support::plain::open_plain_directory;
use crate::support::receipt::{
    EmptyDirReceipt, EntryKind, EntryReceipt, park_entry, publish_entry, receipt_at,
    remove_parked_entry, restore_entry,
};

pub fn replace_alias(
    parent_path: &Path,
    parent: &File,
    name: &str,
    target: &Path,
) -> io::Result<()> {
    let stage = stage(parent_path)?;
    let stage_dir = open_plain_directory(stage.path())?;
    nix::unistd::symlinkat(target, &stage_dir, "next").map_err(io::Error::from)?;
    let next = required_receipt(&stage_dir, "next")?;
    let old = receipt_at(parent, name, EntryKind::Symlink)?;
    if let Some(old) = old {
        park_entry(parent, name, &stage_dir, "old", old, EntryKind::Symlink)?;
    }
    if let Err(error) = publish_entry(&stage_dir, "next", parent, name, next, EntryKind::Symlink) {
        if let Some(old) = old {
            let _restored = restore_entry(&stage_dir, "old", parent, name, old, EntryKind::Symlink);
        }
        return Err(error);
    }
    if let Some(old) = old {
        remove_parked_entry(&stage_dir, "old", old, EntryKind::Symlink)?;
    }
    stage
        .cleanup()
        .map_err(|_error| io::Error::other("cannot clean alias stage"))
}

pub fn remove_alias(
    parent_path: &Path,
    parent: &File,
    name: &str,
    receipt: EntryReceipt,
) -> io::Result<()> {
    let stage = stage(parent_path)?;
    let stage_dir = open_plain_directory(stage.path())?;
    park_entry(parent, name, &stage_dir, "old", receipt, EntryKind::Symlink)?;
    remove_parked_entry(&stage_dir, "old", receipt, EntryKind::Symlink)?;
    stage
        .cleanup()
        .map_err(|_error| io::Error::other("cannot clean alias stage"))
}

fn stage(parent: &Path) -> io::Result<EmptyDirReceipt> {
    let uid = nix::unistd::getuid().as_raw();
    let gid = nix::unistd::getgid().as_raw();
    EmptyDirReceipt::create(parent, ".cortexfs-alias", uid, gid, 0o700)
        .map_err(|_error| io::Error::other("cannot create alias stage"))
}

fn required_receipt(parent: &File, name: &str) -> io::Result<EntryReceipt> {
    receipt_at(parent, name, EntryKind::Symlink)?
        .ok_or_else(|| io::Error::other("created alias disappeared"))
}
