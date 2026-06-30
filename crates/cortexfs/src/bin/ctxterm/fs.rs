fn open_log(path: &Path) -> Result<fs::File, CtxtermError> {
    if let Some(parent) = path.parent() {
        create_ctxterm_plain_dir(parent).map_err(|error| {
            CtxtermError::unavailable(format!("cannot create {}: {error}", parent.display()))
        })?;
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK | nix::libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| {
            CtxtermError::unavailable(format!("cannot open {}: {error}", path.display()))
        })?;
    if !file
        .metadata()
        .map_err(|error| {
            CtxtermError::unavailable(format!("cannot inspect {}: {error}", path.display()))
        })?
        .is_file()
    {
        return Err(CtxtermError::unavailable(format!(
            "{} is not a plain file",
            path.display()
        )));
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|error| {
            CtxtermError::unavailable(format!("cannot chmod {}: {error}", path.display()))
        })?;
    Ok(file)
}

fn create_ctxterm_plain_dir(path: &Path) -> io::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            sync_ctxterm_dir(path)
        } else {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "ctxterm parent path is not a plain directory",
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
                    "ctxterm path contains a non-directory entry",
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
            open_ctxterm_plain_dir(existing_parent)?
        } else {
            return Ok(());
        };

    for directory in missing.iter().rev() {
        let name = directory
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid ctxterm directory name",
                )
            })?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(0o700),
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

fn sync_ctxterm_dir(path: &Path) -> io::Result<()> {
    let directory = open_ctxterm_plain_dir(path)?;
    directory.sync_all()
}

fn open_ctxterm_plain_dir(path: &Path) -> io::Result<fs::File> {
    let mut directory = if path.is_absolute() {
        open_single_ctxterm_plain_dir(Path::new("/"))?
    } else {
        open_single_ctxterm_plain_dir(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "ctxterm path is not utf-8")
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
                .map_err(io::Error::from)?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "ctxterm path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_single_ctxterm_plain_dir(path: &Path) -> io::Result<fs::File> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "ctxterm path is not a directory",
        ));
    }
    Ok(directory)
}
