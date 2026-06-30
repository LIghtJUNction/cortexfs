fn read_provider_secret_file(path: &Path) -> std::io::Result<String> {
    let mut file = open_provider_secret_file(path)?;
    let len = file.metadata()?.len();
    let len = usize::try_from(len)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.utf8_error()))
}

fn open_provider_secret_file(path: &Path) -> std::io::Result<File> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "provider secret has no parent",
        )
    })?;
    let parent_dir = open_plain_directory_no_follow(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid provider secret name",
            )
        })?;
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )?;
    let file = File::from(file_fd);
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_PROVIDER_SYSTEM_SECRET_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "provider secret file is invalid",
        ));
    }
    Ok(file)
}

fn set_private_dir_permissions(path: &Path) -> Result<(), ProviderSystemSecretError> {
    let dir = match open_plain_directory_no_follow(path) {
        Ok(dir) => dir,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_error) => return Err(ProviderSystemSecretError::CannotWrite),
    };
    if !dir
        .metadata()
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)?
        .is_dir()
    {
        return Err(ProviderSystemSecretError::CannotWrite);
    }
    dir.set_permissions(fs::Permissions::from_mode(0o700))
        .and_then(|()| dir.sync_all())
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)
}

fn create_private_provider_secret_dir(path: &Path) -> Result<(), ProviderSystemSecretError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            set_private_dir_permissions(path)
        } else {
            Err(ProviderSystemSecretError::CannotWrite)
        };
    }

    let mut missing = Vec::new();
    let mut cursor = Some(path);
    while let Some(current) = cursor {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
                return Err(ProviderSystemSecretError::CannotWrite);
            }
            Ok(_metadata) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                cursor = current.parent();
            }
            Err(_error) => return Err(ProviderSystemSecretError::CannotWrite),
        }
    }

    let parent = missing
        .last()
        .and_then(|path| path.parent())
        .ok_or(ProviderSystemSecretError::CannotWrite)?;
    let mut parent_dir = open_plain_directory_no_follow(parent)
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
    for directory in missing.iter().rev() {
        let name = plain_file_name(directory).ok_or(ProviderSystemSecretError::CannotWrite)?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(0o700),
        )
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
        parent_dir
            .sync_all()
            .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
        let child = nix::fcntl::openat(
            &parent_dir,
            name,
            nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
        parent_dir = File::from(child);
        parent_dir
            .set_permissions(fs::Permissions::from_mode(0o700))
            .and_then(|()| parent_dir.sync_all())
            .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
    }
    Ok(())
}

fn sync_provider_secret_dir(path: &Path) -> Result<(), ProviderSystemSecretError> {
    let dir = open_plain_directory_no_follow(path)
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)?;
    dir.sync_all()
        .map_err(|_error| ProviderSystemSecretError::CannotWrite)
}

fn open_plain_directory_no_follow(path: &Path) -> std::io::Result<File> {
    let mut directory = if path.is_absolute() {
        open_plain_directory_no_follow_leaf(Path::new("/"))?
    } else {
        open_plain_directory_no_follow_leaf(Path::new("."))?
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
                )?;
                directory = File::from(next);
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

fn open_plain_directory_no_follow_leaf(path: &Path) -> std::io::Result<File> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ));
    }
    Ok(directory)
}

fn plain_file_name(path: &Path) -> Option<&str> {
    path.file_name()?.to_str()
}

fn clear_fd_cloexec(file: &File) -> Result<(), ProviderSystemSecretError> {
    let flags =
        fcntl(file, FcntlArg::F_GETFD).map_err(|_error| ProviderSystemSecretError::CannotRead)?;
    let mut flags = FdFlag::from_bits_truncate(flags);
    flags.remove(FdFlag::FD_CLOEXEC);
    fcntl(file, FcntlArg::F_SETFD(flags))
        .map(|_value| ())
        .map_err(|_error| ProviderSystemSecretError::CannotRead)
}

fn provider_env_label(provider: &str) -> String {
    provider
        .bytes()
        .map(|byte| match byte {
            b'a'..=b'z' => char::from(byte.to_ascii_uppercase()),
            b'A'..=b'Z' | b'0'..=b'9' => char::from(byte),
            _ => '_',
        })
        .collect()
}
