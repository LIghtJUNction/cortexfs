fn read_small_plain_text_file(path: &Path) -> io::Result<String> {
    let mut file = open_plain_read_file(path)?;
    let metadata = file.metadata()?;
    if metadata.len() > MAX_RUNNER_CONTROL_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds runner control read limit",
        ));
    }
    let len = usize::try_from(metadata.len()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file is too large to read: {error}"),
        )
    })?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.utf8_error()))
}

fn open_plain_read_file(path: &Path) -> io::Result<fs::File> {
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
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW,
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

fn open_plain_directory(path: &Path) -> io::Result<fs::File> {
    let mut directory = if path.is_absolute() {
        open_single_plain_directory(Path::new("/"))?
    } else {
        open_single_plain_directory(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid directory name")
                })?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_single_plain_directory(path: &Path) -> io::Result<fs::File> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ));
    }
    Ok(directory)
}

fn is_regular_file_no_follow(path: &Path) -> bool {
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(parent_dir) = open_plain_directory(parent) else {
        return false;
    };
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let Ok(file_fd) = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_PATH | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    ) else {
        return false;
    };
    fs::File::from(file_fd)
        .metadata()
        .is_ok_and(|metadata| metadata.is_file())
}

fn open_executable_no_follow(path: &Path) -> io::Result<fs::File> {
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
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW,
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

fn proc_fd_path(file: &fs::File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()))
}
