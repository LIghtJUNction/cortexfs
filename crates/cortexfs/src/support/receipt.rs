use std::fmt;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{Read, Result};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use super::plain::{open_plain_directory, proc_fd_path};

const QUARANTINE_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SocketReceiptError {
    Create,
    Cleanup,
}

impl fmt::Display for SocketReceiptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match *self {
            Self::Create => "cannot create socket receipt",
            Self::Cleanup => "socket receipt cleanup conflict",
        })
    }
}

impl std::error::Error for SocketReceiptError {}

pub(crate) struct SocketReceipt {
    path: PathBuf,
    dev: u64,
    ino: u64,
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
        if let Err(error) = configure(directory, &parent, parent_identity, name, identity, owner) {
            let _cleanup = quarantine(&parent, name, identity);
            return Err(error);
        }
        Ok((
            Self {
                path,
                dev: identity.0,
                ino: identity.1,
            },
            listener,
        ))
    }

    pub(crate) fn cleanup(&self) -> std::result::Result<(), SocketReceiptError> {
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
mod tests {
    use std::fs;
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt, symlink};

    use super::*;

    fn fixture() -> std::result::Result<
        (tempfile::TempDir, SocketReceipt, UnixListener),
        Box<dyn std::error::Error>,
    > {
        let root = tempfile::tempdir()?;
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o711))?;
        let (receipt, listener) = SocketReceipt::bind(
            root.path(),
            "control.sock",
            (
                nix::unistd::getuid().as_raw(),
                nix::unistd::getgid().as_raw(),
            ),
        )?;
        Ok((root, receipt, listener))
    }

    #[test]
    fn socket_receipt_configures_and_cleans_socket()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let (_root, receipt, listener) = fixture()?;
        let metadata = fs::symlink_metadata(receipt.path())?;
        assert!(metadata.file_type().is_socket());
        assert_eq!(metadata.uid(), nix::unistd::getuid().as_raw());
        assert_eq!(metadata.gid(), nix::unistd::getgid().as_raw());
        assert_eq!(metadata.permissions().mode() & 0o7777, 0o600);
        assert_eq!((metadata.dev(), metadata.ino()), receipt.identity());
        drop(listener);
        receipt.cleanup()?;
        assert!(!receipt.path().exists());
        Ok(())
    }

    #[test]
    fn cleanup_refuses_replacement_socket() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let (_root, receipt, listener) = fixture()?;
        drop(listener);
        fs::remove_file(receipt.path())?;
        let _replacement = UnixListener::bind(receipt.path())?;
        assert_eq!(receipt.cleanup(), Err(SocketReceiptError::Cleanup));
        assert!(receipt.path().exists());
        Ok(())
    }

    #[test]
    fn cleanup_refuses_non_socket_and_symlink()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        for use_symlink in [false, true] {
            let (_root, receipt, listener) = fixture()?;
            drop(listener);
            fs::remove_file(receipt.path())?;
            if use_symlink {
                symlink("missing", receipt.path())?;
            } else {
                fs::write(receipt.path(), b"replacement")?;
            }
            assert_eq!(receipt.cleanup(), Err(SocketReceiptError::Cleanup));
            assert!(fs::symlink_metadata(receipt.path()).is_ok());
        }
        Ok(())
    }

    #[test]
    fn random_hex_has_exact_lowercase_format() -> std::result::Result<(), Box<dyn std::error::Error>>
    {
        let token = random_hex::<32>()?;
        assert_eq!(token.len(), 64);
        assert!(
            token
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        );
        Ok(())
    }
}
