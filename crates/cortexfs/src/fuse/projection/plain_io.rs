fn fuse_file_type_from_mode(mode: libc::mode_t) -> FuseV1FileType {
    match mode & libc::S_IFMT {
        libc::S_IFDIR => FuseV1FileType::Directory,
        libc::S_IFREG => FuseV1FileType::Regular,
        libc::S_IFLNK => FuseV1FileType::Symlink,
        libc::S_IFSOCK => FuseV1FileType::Socket,
        _ => FuseV1FileType::Other,
    }
}

fn fuse_v1_plain_dir_exists(path: &Path) -> Result<bool, FuseV1Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(true),
        Ok(_metadata) => Err(FuseV1Error::Io),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_error) => Err(FuseV1Error::Io),
    }
}

fn read_fuse_v1_symlink_target(path: &Path) -> std::io::Result<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_fuse_v1_plain_directory(parent)?;
    let file_name = fuse_v1_plain_file_name(path)?;
    nix::fcntl::readlinkat(&parent_dir, file_name)
        .map(PathBuf::from)
        .map_err(std::io::Error::from)
}

fn read_fuse_v1_small_text_file(path: &Path, max_bytes: u64) -> std::io::Result<String> {
    let mut file = open_fuse_v1_plain_file(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "path is not a regular file",
        ));
    }
    if metadata.len() > max_bytes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file exceeds read limit",
        ));
    }
    let len = usize::try_from(metadata.len()).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("file is too large to read: {error}"),
        )
    })?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.utf8_error()))
}

fn fuse_v1_plain_path_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_fuse_v1_plain_directory(parent)?;
    let file_name = fuse_v1_plain_file_name(path)?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_PATH | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    fs::File::from(file_fd).metadata()
}

fn open_fuse_v1_plain_file(path: &Path) -> std::io::Result<fs::File> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_dir = open_fuse_v1_plain_directory(parent)?;
    let file_name = fuse_v1_plain_file_name(path)?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    Ok(fs::File::from(file_fd))
}

fn fuse_v1_plain_file_name(path: &Path) -> std::io::Result<&str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid file name"))
}

fn open_fuse_v1_plain_directory(path: &Path) -> std::io::Result<fs::File> {
    let mut directory = if path.is_absolute() {
        open_fuse_v1_single_plain_directory(Path::new("/"))?
    } else {
        open_fuse_v1_single_plain_directory(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid directory name")
                })?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )
                .map_err(std::io::Error::from)?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_fuse_v1_single_plain_directory(path: &Path) -> std::io::Result<fs::File> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ));
    }
    Ok(directory)
}

fn create_fuse_v1_plain_dir(path: &Path) -> Result<(), FuseV1Error> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            sync_fuse_v1_dir(path)
        } else {
            Err(FuseV1Error::Io)
        };
    }

    let mut missing = Vec::new();
    let mut cursor = Some(path);
    while let Some(current) = cursor {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
                return Err(FuseV1Error::Io);
            }
            Ok(_metadata) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                cursor = current.parent();
            }
            Err(_error) => return Err(FuseV1Error::Io),
        }
    }

    let existing_parent = missing
        .last()
        .and_then(|path| path.parent())
        .ok_or(FuseV1Error::Io)?;
    let mut parent_dir = open_fuse_v1_plain_directory(existing_parent)
        .map_err(|_error| FuseV1Error::Io)?;
    for directory in missing.iter().rev() {
        let name = fuse_v1_plain_file_name(directory).map_err(|_error| FuseV1Error::Io)?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(0o755),
        )
        .map_err(|_error| FuseV1Error::Io)?;
        parent_dir.sync_all().map_err(|_error| FuseV1Error::Io)?;
        let child = nix::fcntl::openat(
            &parent_dir,
            name,
            nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(|_error| FuseV1Error::Io)?;
        parent_dir = fs::File::from(child);
        parent_dir.sync_all().map_err(|_error| FuseV1Error::Io)?;
    }
    Ok(())
}

fn sync_fuse_v1_dir(path: &Path) -> Result<(), FuseV1Error> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_error| FuseV1Error::Io)?;
    if !directory
        .metadata()
        .map_err(|_error| FuseV1Error::Io)?
        .is_dir()
    {
        return Err(FuseV1Error::Io);
    }
    directory.sync_all().map_err(|_error| FuseV1Error::Io)
}
