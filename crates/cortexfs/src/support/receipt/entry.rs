use std::fs::File;
use std::io;

use nix::fcntl::AtFlags;
use nix::libc;
use nix::sys::stat::{SFlag, fstatat};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntryKind {
    Directory,
    File,
    Executable,
    Symlink,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntryReceipt {
    pub(crate) dev: u64,
    pub(crate) ino: u64,
}

pub fn receipt_at(parent: &File, name: &str, kind: EntryKind) -> io::Result<Option<EntryReceipt>> {
    match fstatat(parent, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) if kind_matches(stat.st_mode, kind) => Ok(Some(EntryReceipt {
            dev: stat.st_dev,
            ino: stat.st_ino,
        })),
        Ok(_) => Err(io::Error::from(io::ErrorKind::InvalidInput)),
        Err(nix::errno::Errno::ENOENT) => Ok(None),
        Err(error) => Err(io::Error::from(error)),
    }
}

pub fn entry_matches(parent: &File, name: &str, receipt: EntryReceipt, kind: EntryKind) -> bool {
    receipt_at(parent, name, kind).is_ok_and(|actual| actual == Some(receipt))
}

fn kind_matches(mode: libc::mode_t, kind: EntryKind) -> bool {
    let actual = SFlag::from_bits_truncate(mode);
    match kind {
        EntryKind::Directory => actual == SFlag::S_IFDIR,
        EntryKind::File => actual == SFlag::S_IFREG,
        EntryKind::Executable => actual == SFlag::S_IFREG && mode & 0o111 != 0,
        EntryKind::Symlink => actual == SFlag::S_IFLNK,
    }
}
