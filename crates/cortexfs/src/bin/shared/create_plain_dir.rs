fn create_plain_directory(
    path: &Path,
    mode: u32,
    existing_not_dir_message: &'static str,
    contains_non_dir_message: &'static str,
    invalid_name_message: &'static str,
) -> io::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            open_plain_directory(path)?.sync_all()
        } else {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                existing_not_dir_message,
            ))
        };
    }

    let mut missing = Vec::new();
    let mut cursor = Some(path);
    while let Some(current) = cursor {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    contains_non_dir_message,
                ));
            }
            Ok(_metadata) => break,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                cursor = current.parent();
            }
            Err(error) => return Err(error),
        }
    }

    let mut parent_dir =
        if let Some(existing_parent) = missing.last().and_then(|path| path.parent()) {
            open_plain_directory(existing_parent)?
        } else {
            return Ok(());
        };

    for directory in missing.iter().rev() {
        let name = directory
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, invalid_name_message))?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(mode),
        )?;
        parent_dir.sync_all()?;
        let child = nix::fcntl::openat(
            &parent_dir,
            name,
            nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )?;
        parent_dir = fs::File::from(child);
        parent_dir.sync_all()?;
    }
    Ok(())
}
