use std::fs;

use super::write::remove_temp;
use super::{AtomicReplaceOutcome, publish_outcome};
use crate::support::plain::is_fuse;

pub(super) fn commit_preserving(
    parent: &fs::File,
    temp: &str,
    name: &str,
    expected: (u64, u64),
    replacement: (u64, u64),
) -> std::io::Result<AtomicReplaceOutcome> {
    if is_fuse(parent)? {
        // Synthetic inodes are path-derived; the mount enforces owner-UID writes.
        if !target_matches(parent, name, expected) {
            remove_temp(parent, temp);
            return Err(std::io::Error::other("atomic replace target changed"));
        }
        if let Err(error) = nix::fcntl::renameat(parent, temp, parent, name) {
            remove_temp(parent, temp);
            return Err(error.into());
        }
        return Ok(publish_outcome(parent.sync_all()));
    }

    nix::fcntl::renameat2(
        parent,
        temp,
        parent,
        name,
        nix::fcntl::RenameFlags::RENAME_EXCHANGE,
    )?;
    if target_matches(parent, temp, expected) {
        remove_temp(parent, temp);
        return Ok(publish_outcome(parent.sync_all()));
    }
    if !target_matches(parent, name, replacement) {
        return Err(std::io::Error::other(
            "atomic replace target changed during rollback",
        ));
    }
    nix::fcntl::renameat2(
        parent,
        temp,
        parent,
        name,
        nix::fcntl::RenameFlags::RENAME_EXCHANGE,
    )?;
    remove_temp(parent, temp);
    let _ignored = parent.sync_all();
    Err(std::io::Error::other("atomic replace target changed"))
}

fn target_matches(parent: &fs::File, name: &str, identity: (u64, u64)) -> bool {
    nix::sys::stat::fstatat(parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW).is_ok_and(
        |stat| {
            nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
                .contains(nix::sys::stat::SFlag::S_IFREG)
                && (stat.st_dev, stat.st_ino) == identity
        },
    )
}
