#![expect(
    unreachable_pub,
    reason = "private receipt submodules expose items only through crate-visible reexports"
)]
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "staged empty-directory receipt awaits its phase-specific caller"
    )
)]

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{Read, Result};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use super::plain::{open_directory_at, open_plain_directory, proc_fd_path};

mod entry;
#[cfg(test)]
mod hook;
mod park;
pub(crate) use entry::{EntryKind, EntryReceipt, entry_matches, receipt_at};
#[cfg(test)]
pub(crate) use hook::{ParkHook, set_park_hook};
pub(crate) use park::{park_entry, publish_entry, remove_parked_entry, restore_entry};

const QUARANTINE_BYTES: usize = 32;
const CHILD_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum ReceiptError {
    #[error("cannot create directory receipt")]
    CannotCreate,
    #[error("directory receipt cleanup conflict")]
    CleanupConflict,
}

pub(crate) struct EmptyDirReceipt {
    path: PathBuf,
    parent: (u64, u64),
    child: (u64, u64),
    child_fd: File,
}

impl EmptyDirReceipt {
    pub(crate) fn create(
        directory: &Path,
        prefix: &str,
        uid: u32,
        gid: u32,
        mode: u32,
    ) -> std::result::Result<Self, ReceiptError> {
        if prefix.is_empty() || Path::new(prefix).file_name() != Some(prefix.as_ref()) {
            return Err(ReceiptError::CannotCreate);
        }
        let parent =
            open_plain_directory(directory).map_err(|_error| ReceiptError::CannotCreate)?;
        let metadata = parent
            .metadata()
            .map_err(|_error| ReceiptError::CannotCreate)?;
        let parent_identity = (metadata.dev(), metadata.ino());
        let suffix = random_hex::<CHILD_BYTES>().map_err(|_error| ReceiptError::CannotCreate)?;
        let name = format!("{prefix}-{}", suffix.get(..24).unwrap_or(&suffix));
        nix::sys::stat::mkdirat(
            &parent,
            name.as_str(),
            nix::sys::stat::Mode::from_bits_truncate(mode),
        )
        .map_err(|_error| ReceiptError::CannotCreate)?;
        let child = match plain_dir_identity(&parent, &name) {
            Ok(identity) => identity,
            Err(_error) => {
                let _isolated = isolate_unidentified_dir(&parent, &name);
                return Err(ReceiptError::CannotCreate);
            }
        };
        let child_fd = match open_directory_at(&parent, OsStr::new(&name)) {
            Ok(file)
                if file
                    .metadata()
                    .is_ok_and(|metadata| (metadata.dev(), metadata.ino()) == child) =>
            {
                file
            }
            Ok(_) | Err(_) => {
                let _cleanup = quarantine_dir(&parent, &name, child);
                return Err(ReceiptError::CannotCreate);
            }
        };
        if configure_dir(
            directory,
            &parent,
            &name,
            (parent_identity, child),
            (uid, gid),
            mode,
        )
        .is_err()
        {
            let _cleanup = quarantine_dir(&parent, &name, child);
            return Err(ReceiptError::CannotCreate);
        }
        Ok(Self {
            path: directory.join(name),
            parent: parent_identity,
            child,
            child_fd,
        })
    }

    pub(crate) fn cleanup(&self) -> std::result::Result<(), ReceiptError> {
        let child_metadata = self
            .child_fd
            .metadata()
            .map_err(|_error| ReceiptError::CleanupConflict)?;
        if (child_metadata.dev(), child_metadata.ino()) != self.child {
            return Err(ReceiptError::CleanupConflict);
        }
        let parent_path = self.path.parent().ok_or(ReceiptError::CleanupConflict)?;
        let parent =
            open_plain_directory(parent_path).map_err(|_error| ReceiptError::CleanupConflict)?;
        let metadata = parent
            .metadata()
            .map_err(|_error| ReceiptError::CleanupConflict)?;
        if (metadata.dev(), metadata.ino()) != self.parent {
            return Err(ReceiptError::CleanupConflict);
        }
        let name = self
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(ReceiptError::CleanupConflict)?;
        quarantine_dir(&parent, name, self.child)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

fn configure_dir(
    directory_path: &Path,
    directory: &File,
    name: &str,
    identities: ((u64, u64), (u64, u64)),
    owner: (u32, u32),
    mode: u32,
) -> std::result::Result<(), ReceiptError> {
    let (parent_identity, child_identity) = identities;
    require_plain_dir(directory, name, child_identity)?;
    nix::unistd::fchownat(
        directory,
        name,
        Some(nix::unistd::Uid::from_raw(owner.0)),
        Some(nix::unistd::Gid::from_raw(owner.1)),
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| ReceiptError::CannotCreate)?;
    require_plain_dir(directory, name, child_identity)?;
    nix::sys::stat::fchmodat(
        directory,
        name,
        nix::sys::stat::Mode::from_bits_truncate(mode),
        nix::sys::stat::FchmodatFlags::NoFollowSymlink,
    )
    .map_err(|_error| ReceiptError::CannotCreate)?;
    require_plain_dir(directory, name, child_identity)?;
    let rebound =
        open_plain_directory(directory_path).map_err(|_error| ReceiptError::CannotCreate)?;
    let metadata = rebound
        .metadata()
        .map_err(|_error| ReceiptError::CannotCreate)?;
    if (metadata.dev(), metadata.ino()) != parent_identity {
        return Err(ReceiptError::CannotCreate);
    }
    directory
        .sync_all()
        .map_err(|_error| ReceiptError::CannotCreate)
}

fn plain_dir_identity(parent: &File, name: &str) -> std::result::Result<(u64, u64), ReceiptError> {
    let stat = nix::sys::stat::fstatat(parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .map_err(|_error| ReceiptError::CleanupConflict)?;
    let kind = nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode);
    if kind != nix::sys::stat::SFlag::S_IFDIR {
        return Err(ReceiptError::CleanupConflict);
    }
    Ok((stat.st_dev, stat.st_ino))
}

fn require_plain_dir(
    parent: &File,
    name: &str,
    expected: (u64, u64),
) -> std::result::Result<(), ReceiptError> {
    if plain_dir_identity(parent, name)? == expected {
        Ok(())
    } else {
        Err(ReceiptError::CleanupConflict)
    }
}

fn require_empty_dir(parent: &File, name: &str) -> std::result::Result<(), ReceiptError> {
    let fd = nix::fcntl::openat(
        parent,
        name,
        nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_DIRECTORY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|_error| ReceiptError::CleanupConflict)?;
    let child = File::from(fd);
    let mut entries =
        std::fs::read_dir(proc_fd_path(&child)).map_err(|_error| ReceiptError::CleanupConflict)?;
    if entries.next().is_some() {
        return Err(ReceiptError::CleanupConflict);
    }
    Ok(())
}

fn quarantine_dir(
    parent: &File,
    name: &str,
    expected: (u64, u64),
) -> std::result::Result<(), ReceiptError> {
    require_plain_dir(parent, name, expected)?;
    require_empty_dir(parent, name)?;
    let quarantine = dir_quarantine_name(name)?;
    nix::fcntl::renameat2(
        parent,
        name,
        parent,
        quarantine.as_str(),
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|_error| ReceiptError::CleanupConflict)?;
    if require_plain_dir(parent, &quarantine, expected).is_err()
        || require_empty_dir(parent, &quarantine).is_err()
    {
        rollback_dir(parent, &quarantine, name, expected);
        return Err(ReceiptError::CleanupConflict);
    }
    if nix::unistd::unlinkat(
        parent,
        quarantine.as_str(),
        nix::unistd::UnlinkatFlags::RemoveDir,
    )
    .is_err()
    {
        rollback_dir(parent, &quarantine, name, expected);
        return Err(ReceiptError::CleanupConflict);
    }
    parent
        .sync_all()
        .map_err(|_error| ReceiptError::CleanupConflict)
}

fn rollback_dir(parent: &File, quarantine: &str, name: &str, expected: (u64, u64)) {
    if require_plain_dir(parent, quarantine, expected).is_ok() {
        let _ignored = nix::fcntl::renameat2(
            parent,
            quarantine,
            parent,
            name,
            nix::fcntl::RenameFlags::RENAME_NOREPLACE,
        );
    }
}

fn isolate_unidentified_dir(parent: &File, name: &str) -> std::result::Result<(), ReceiptError> {
    let quarantine = dir_quarantine_name(name)?;
    nix::fcntl::renameat2(
        parent,
        name,
        parent,
        quarantine.as_str(),
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|_error| ReceiptError::CleanupConflict)?;
    parent
        .sync_all()
        .map_err(|_error| ReceiptError::CleanupConflict)
}

fn dir_quarantine_name(name: &str) -> std::result::Result<String, ReceiptError> {
    let suffix =
        random_hex::<QUARANTINE_BYTES>().map_err(|_error| ReceiptError::CleanupConflict)?;
    Ok(format!(
        ".{name}.rollback-{}",
        suffix.get(..16).unwrap_or(&suffix)
    ))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub(crate) enum SocketReceiptError {
    #[error("cannot create socket receipt")]
    Create,
    #[error("socket receipt cleanup conflict")]
    Cleanup,
}

pub(crate) struct SocketReceipt {
    path: PathBuf,
    dev: u64,
    ino: u64,
    socket_fd: File,
}

impl SocketReceipt {
    pub(crate) fn bind(
        directory: &Path,
        name: &str,
        owner: (u32, u32),
    ) -> std::result::Result<(Self, UnixListener), SocketReceiptError> {
        let parent =
            open_plain_directory(directory).map_err(|_error| SocketReceiptError::Create)?;
        let parent_metadata = parent
            .metadata()
            .map_err(|_error| SocketReceiptError::Create)?;
        if !parent_metadata.is_dir()
            || parent_metadata.file_type().is_symlink()
            || parent_metadata.uid() != nix::unistd::geteuid().as_raw()
            || parent_metadata.mode() & 0o7777 != 0o711
        {
            return Err(SocketReceiptError::Create);
        }
        let parent_identity = (parent_metadata.dev(), parent_metadata.ino());
        let path = directory.join(name);
        let listener = UnixListener::bind(proc_fd_path(&parent).join(name))
            .map_err(|_error| SocketReceiptError::Create)?;
        let identity = match socket_identity(&parent, name) {
            Ok(identity) => identity,
            Err(error) => {
                let _isolated = isolate_unidentified(&parent, name);
                return Err(error);
            }
        };
        let socket_fd = match open_socket_at(&parent, name, identity) {
            Ok(file) => file,
            Err(error) => {
                let _cleanup = quarantine(&parent, name, identity);
                return Err(error);
            }
        };
        if let Err(error) = configure(directory, &parent, parent_identity, name, identity, owner) {
            let _cleanup = quarantine(&parent, name, identity);
            return Err(error);
        }
        Ok((
            Self {
                path,
                dev: identity.0,
                ino: identity.1,
                socket_fd,
            },
            listener,
        ))
    }

    pub(crate) fn cleanup(&self) -> std::result::Result<(), SocketReceiptError> {
        let metadata = self
            .socket_fd
            .metadata()
            .map_err(|_error| SocketReceiptError::Cleanup)?;
        if (metadata.dev(), metadata.ino()) != (self.dev, self.ino) {
            return Err(SocketReceiptError::Cleanup);
        }
        let parent_path = self.path.parent().ok_or(SocketReceiptError::Cleanup)?;
        let parent =
            open_plain_directory(parent_path).map_err(|_error| SocketReceiptError::Cleanup)?;
        let name = self
            .path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(SocketReceiptError::Cleanup)?;
        quarantine(&parent, name, (self.dev, self.ino))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) const fn identity(&self) -> (u64, u64) {
        (self.dev, self.ino)
    }
}

fn configure(
    directory_path: &Path,
    directory: &File,
    directory_identity: (u64, u64),
    name: &str,
    identity: (u64, u64),
    owner: (u32, u32),
) -> std::result::Result<(), SocketReceiptError> {
    let rebound =
        open_plain_directory(directory_path).map_err(|_error| SocketReceiptError::Create)?;
    let metadata = rebound
        .metadata()
        .map_err(|_error| SocketReceiptError::Create)?;
    if directory_identity != (metadata.dev(), metadata.ino()) {
        return Err(SocketReceiptError::Create);
    }
    require_identity(directory, name, identity)?;
    nix::sys::stat::fchmodat(
        directory,
        name,
        nix::sys::stat::Mode::from_bits_truncate(0o600),
        nix::sys::stat::FchmodatFlags::NoFollowSymlink,
    )
    .map_err(|_error| SocketReceiptError::Create)?;
    require_identity(directory, name, identity)?;
    nix::unistd::fchownat(
        directory,
        name,
        Some(nix::unistd::Uid::from_raw(owner.0)),
        Some(nix::unistd::Gid::from_raw(owner.1)),
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| SocketReceiptError::Create)?;
    require_identity(directory, name, identity)?;
    directory
        .sync_all()
        .map_err(|_error| SocketReceiptError::Create)
}

fn socket_identity(
    parent: &File,
    name: &str,
) -> std::result::Result<(u64, u64), SocketReceiptError> {
    let stat = nix::sys::stat::fstatat(parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .map_err(|_error| SocketReceiptError::Create)?;
    let kind = nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode);
    if !kind.contains(nix::sys::stat::SFlag::S_IFSOCK) {
        return Err(SocketReceiptError::Create);
    }
    Ok((stat.st_dev, stat.st_ino))
}

fn open_socket_at(
    parent: &File,
    name: &str,
    expected: (u64, u64),
) -> std::result::Result<File, SocketReceiptError> {
    let fd = nix::fcntl::openat(
        parent,
        name,
        nix::fcntl::OFlag::O_PATH | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(|_error| SocketReceiptError::Create)?;
    let file = File::from(fd);
    let metadata = file
        .metadata()
        .map_err(|_error| SocketReceiptError::Create)?;
    let kind = nix::sys::stat::SFlag::from_bits_truncate(metadata.mode());
    if (metadata.dev(), metadata.ino()) != expected
        || !kind.contains(nix::sys::stat::SFlag::S_IFSOCK)
    {
        return Err(SocketReceiptError::Create);
    }
    Ok(file)
}

fn require_identity(
    parent: &File,
    name: &str,
    expected: (u64, u64),
) -> std::result::Result<(), SocketReceiptError> {
    if socket_identity(parent, name)? == expected {
        Ok(())
    } else {
        Err(SocketReceiptError::Cleanup)
    }
}

fn quarantine(
    parent: &File,
    name: &str,
    expected: (u64, u64),
) -> std::result::Result<(), SocketReceiptError> {
    require_identity(parent, name, expected).map_err(|_error| SocketReceiptError::Cleanup)?;
    let quarantine = quarantine_name(name)?;
    nix::fcntl::renameat2(
        parent,
        name,
        parent,
        quarantine.as_str(),
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|_error| SocketReceiptError::Cleanup)?;
    if require_identity(parent, &quarantine, expected).is_err() {
        let _ignored = nix::fcntl::renameat2(
            parent,
            quarantine.as_str(),
            parent,
            name,
            nix::fcntl::RenameFlags::RENAME_NOREPLACE,
        );
        return Err(SocketReceiptError::Cleanup);
    }
    nix::unistd::unlinkat(
        parent,
        quarantine.as_str(),
        nix::unistd::UnlinkatFlags::NoRemoveDir,
    )
    .map_err(|_error| SocketReceiptError::Cleanup)?;
    parent
        .sync_all()
        .map_err(|_error| SocketReceiptError::Cleanup)
}

fn isolate_unidentified(parent: &File, name: &str) -> std::result::Result<(), SocketReceiptError> {
    let quarantine = quarantine_name(name)?;
    nix::fcntl::renameat2(
        parent,
        name,
        parent,
        quarantine.as_str(),
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|_error| SocketReceiptError::Cleanup)?;
    parent
        .sync_all()
        .map_err(|_error| SocketReceiptError::Cleanup)
}

fn quarantine_name(name: &str) -> std::result::Result<String, SocketReceiptError> {
    let suffix = random_hex::<QUARANTINE_BYTES>().map_err(|_error| SocketReceiptError::Cleanup)?;
    Ok(format!(
        ".{name}.rollback-{}",
        suffix.get(..16).unwrap_or(&suffix)
    ))
}

/// Reads system entropy and returns exactly `N * 2` lowercase hexadecimal bytes.
pub(crate) fn random_hex<const N: usize>() -> Result<String> {
    let mut bytes = [0_u8; N];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let mut token = String::with_capacity(N * 2);
    for byte in bytes {
        write!(&mut token, "{byte:02x}").map_err(std::io::Error::other)?;
    }
    Ok(token)
}

#[cfg(test)]
mod tests;
