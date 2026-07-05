fn open_regular_file_no_follow(path: &Path, extra_flags: nix::fcntl::OFlag) -> io::Result<fs::File> {
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "path has no parent directory")
    })?;
    let parent_dir = open_plain_directory(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | extra_flags,
        nix::sys::stat::Mode::empty(),
    )?;
    let file = fs::File::from(file_fd);
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "path is not a regular file",
        ));
    }
    Ok(file)
}

fn open_executable_no_follow(path: &Path) -> io::Result<fs::File> {
    open_regular_file_no_follow(path, nix::fcntl::OFlag::empty())
}
