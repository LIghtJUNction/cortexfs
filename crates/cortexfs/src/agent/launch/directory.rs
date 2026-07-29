use std::fs;
use std::io;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;

use crate::AgentUnixIdentity;

pub(crate) fn ensure_terminal_runtime_dir(
    runtime: &Path,
    agent: &str,
    session: &str,
    identity: &AgentUnixIdentity,
) -> io::Result<fs::File> {
    let mut parent = crate::support::plain::open_plain_directory(runtime)?;
    let runtime_meta = parent.metadata()?;
    if runtime_meta.uid() != identity.uid() || runtime_meta.permissions().mode() & 0o7777 != 0o700 {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    for (name, mode) in [
        ("cortexfs", 0o755),
        ("terminal", 0o700),
        (agent, 0o700),
        (session, 0o700),
    ] {
        if !crate::is_object_name(name) {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let created = match nix::sys::stat::mkdirat(
            &parent,
            name,
            nix::sys::stat::Mode::from_bits_truncate(mode),
        ) {
            Ok(()) => true,
            Err(nix::errno::Errno::EEXIST) => false,
            Err(error) => return Err(io::Error::from(error)),
        };
        let fd = nix::fcntl::openat(
            &parent,
            name,
            nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map(fs::File::from)
        .map_err(io::Error::from)?;
        if created {
            nix::unistd::fchown(
                &fd,
                Some(nix::unistd::Uid::from_raw(identity.uid())),
                Some(nix::unistd::Gid::from_raw(identity.gid())),
            )
            .map_err(io::Error::from)?;
            nix::sys::stat::fchmod(&fd, nix::sys::stat::Mode::from_bits_truncate(mode))
                .map_err(io::Error::from)?;
        }
        let metadata = fd.metadata()?;
        if metadata.uid() != identity.uid()
            || metadata.gid() != identity.gid()
            || metadata.permissions().mode() & 0o7777 != mode
        {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        parent = fd;
    }
    Ok(parent)
}
