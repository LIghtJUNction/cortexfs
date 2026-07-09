use crate::*;

pub(crate) fn remove_stale_socket(socket: &Path) -> io::Result<()> {
    let parent = socket.parent().unwrap_or_else(|| Path::new("."));
    let parent = open_plain_directory(parent)?;
    let file_name = socket
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid socket name"))?;
    match nix::sys::stat::fstatat(&parent, file_name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat)
            if nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
                .contains(nix::sys::stat::SFlag::S_IFSOCK) =>
        {
            nix::unistd::unlinkat(&parent, file_name, nix::unistd::UnlinkatFlags::NoRemoveDir)
                .map_err(io::Error::from)
        }
        Ok(_metadata) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "refusing to replace non-socket path",
        )),
        Err(nix::errno::Errno::ENOENT) => Ok(()),
        Err(error) => Err(io::Error::from(error)),
    }
}
