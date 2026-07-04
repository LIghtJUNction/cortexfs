fn is_plain_dir_path(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_dir())
}

fn is_plain_file_path(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_file())
}

fn read_plain_text_file(path: &Path) -> std::io::Result<String> {
    let mut file = open_plain_file_no_follow(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::other("path is not a plain file"));
    }
    if metadata.len() > MAX_CONTEXT_PACK_SOURCE_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "context pack source is too large",
        ));
    }
    let len = usize::try_from(metadata.len())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.utf8_error()))
}

fn open_plain_file_no_follow(path: &Path) -> std::io::Result<File> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid file name")
        })?;
    let parent_dir = open_directory_no_symlink_components(parent)?;
    let file = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    Ok(File::from(file))
}

fn open_directory_no_symlink_components(path: &Path) -> std::io::Result<File> {
    let mut directory = if path.is_absolute() {
        open_directory_no_follow(Path::new("/"))?
    } else {
        open_directory_no_follow(Path::new("."))?
    };
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => {
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
                directory = File::from(next);
            }
            Component::ParentDir | Component::Prefix(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_directory_no_follow(path: &Path) -> std::io::Result<File> {
    let directory = OpenOptions::new()
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

fn directory_entry_names(path: &Path) -> Result<Vec<String>, ContextPackBuildError> {
    let directory = open_directory_no_symlink_components(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ContextPackBuildError::MissingSession
        } else {
            ContextPackBuildError::CannotRead
        }
    })?;
    let entries = fs::read_dir(proc_fd_path(&directory)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ContextPackBuildError::MissingSession
        } else {
            ContextPackBuildError::CannotRead
        }
    })?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_error| ContextPackBuildError::CannotRead)?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or(ContextPackBuildError::CannotRead)?
            .to_owned();
        names.push(name);
    }
    Ok(names)
}

fn proc_fd_path(directory: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()))
}

fn is_safe_relative_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.bytes().any(|byte| byte.is_ascii_control())
        && name != "."
        && name != ".."
}

fn estimate_context_tokens(content: &str) -> u64 {
    let words = content.split_whitespace().count();
    u64::try_from(words.max(1)).unwrap_or(u64::MAX)
}
