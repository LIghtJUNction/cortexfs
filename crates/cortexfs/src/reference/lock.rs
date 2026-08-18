use crate::ReferenceTreeError;
use crate::support::plain::{create_plain_dir, open_plain_directory};
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

const PROVIDER_RECONCILE_LOCK: &str = ".cortexfs-provider-reconcile.lock";

pub struct ProviderReconcileLock {
    _directory: fs::File,
    _lock: nix::fcntl::Flock<fs::File>,
}

/// Serializes provider projection with a regular no-follow lock in its cache.
pub fn lock_provider_reconciliation(
    cache_dir: &Path,
) -> Result<ProviderReconcileLock, ReferenceTreeError> {
    create_plain_dir(cache_dir).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let directory =
        open_plain_directory(cache_dir).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let fd = nix::fcntl::openat(
        &directory,
        PROVIDER_RECONCILE_LOCK,
        nix::fcntl::OFlag::O_RDWR
            | nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    )
    .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let file = fs::File::from(fd);
    let before = file
        .metadata()
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    if !before.is_file() || before.file_type().is_symlink() {
        return Err(ReferenceTreeError::CannotCreate);
    }
    let lock = nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusive)
        .map_err(|(_file, _error)| ReferenceTreeError::CannotCreate)?;
    let linked = nix::sys::stat::fstatat(
        &directory,
        PROVIDER_RECONCILE_LOCK,
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let regular = nix::sys::stat::SFlag::from_bits_truncate(linked.st_mode)
        .contains(nix::sys::stat::SFlag::S_IFREG);
    if !regular || (linked.st_dev, linked.st_ino) != (before.dev(), before.ino()) {
        return Err(ReferenceTreeError::CannotCreate);
    }
    Ok(ProviderReconcileLock {
        _directory: directory,
        _lock: lock,
    })
}
