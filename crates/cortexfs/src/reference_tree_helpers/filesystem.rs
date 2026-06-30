fn create_reference_dir(path: &Path) -> Result<(), ReferenceTreeError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            sync_reference_dir(path)
        } else {
            Err(ReferenceTreeError::CannotCreate)
        };
    }

    let mut missing = Vec::new();
    let mut cursor = Some(path);
    while let Some(current) = cursor {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.file_type().is_dir() => {
                return Err(ReferenceTreeError::CannotCreate);
            }
            Ok(_metadata) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                cursor = current.parent();
            }
            Err(_error) => return Err(ReferenceTreeError::CannotCreate),
        }
    }
    let mut parent_dir = if let Some(existing_parent) = missing.last().and_then(|path| path.parent())
    {
        open_reference_dir(existing_parent)?
    } else {
        return Ok(());
    };

    for directory in missing.iter().rev() {
        let name = reference_file_name(directory)?;
        nix::sys::stat::mkdirat(&parent_dir, name, nix::sys::stat::Mode::from_bits_truncate(0o755))
            .map_err(|_error| ReferenceTreeError::CannotCreate)?;
        parent_dir
            .sync_all()
            .map_err(|_error| ReferenceTreeError::CannotCreate)?;
        let child = nix::fcntl::openat(
            &parent_dir,
            name,
            nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
        parent_dir = fs::File::from(child);
        parent_dir
            .sync_all()
            .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    }
    Ok(())
}

fn reference_file_name(path: &Path) -> Result<&str, ReferenceTreeError> {
    path.file_name()
        .and_then(|name| name.to_str())
        .ok_or(ReferenceTreeError::CannotCreate)
}

fn sync_reference_dir(path: &Path) -> Result<(), ReferenceTreeError> {
    let directory = open_reference_dir(path)?;
    directory
        .sync_all()
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn open_reference_dir(path: &Path) -> Result<fs::File, ReferenceTreeError> {
    let mut directory = if path.is_absolute() {
        open_reference_dir_leaf(Path::new("/"))?
    } else {
        open_reference_dir_leaf(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or(ReferenceTreeError::CannotCreate)?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )
                .map_err(|_error| ReferenceTreeError::CannotCreate)?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(ReferenceTreeError::CannotCreate);
            }
        }
    }
    Ok(directory)
}

fn open_reference_dir_leaf(path: &Path) -> Result<fs::File, ReferenceTreeError> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    if !directory
        .metadata()
        .map_err(|_error| ReferenceTreeError::CannotCreate)?
        .is_dir()
    {
        return Err(ReferenceTreeError::CannotCreate);
    }
    Ok(directory)
}

fn ensure_reference_home_ownership(path: &Path) -> Result<(), ReferenceTreeError> {
    if !nix::unistd::Uid::effective().is_root() {
        return Ok(());
    }
    chown_reference_home_entry(path)
}

fn chown_reference_home_entry(path: &Path) -> Result<(), ReferenceTreeError> {
    nix::unistd::fchownat(
        nix::fcntl::AT_FDCWD,
        path,
        Some(nix::unistd::Uid::from_raw(REFERENCE_HOME_UID)),
        Some(nix::unistd::Gid::from_raw(REFERENCE_HOME_GID)),
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn write_reference_text(path: &Path, content: &str) -> Result<(), ReferenceTreeError> {
    if let Some(parent) = path.parent() {
        create_reference_dir(parent)?;
    }
    atomic_replace_text_with_mode(path, content, 0o644)
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn set_reference_executable(path: &Path) -> Result<(), ReferenceTreeError> {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    if !file
        .metadata()
        .map_err(|_error| ReferenceTreeError::CannotCreate)?
        .is_file()
    {
        return Err(ReferenceTreeError::CannotCreate);
    }
    file.set_permissions(fs::Permissions::from_mode(0o755))
        .and_then(|()| file.sync_all())
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn ensure_reference_socket(path: &Path) -> Result<(), ReferenceTreeError> {
    if let Some(parent) = path.parent() {
        create_reference_dir(parent)?;
    }
    let Some(parent) = path.parent() else {
        return Err(ReferenceTreeError::CannotSocket(
            std::io::ErrorKind::InvalidInput,
        ));
    };
    let parent_dir = open_reference_dir(parent).map_err(|_error| {
        ReferenceTreeError::CannotSocket(std::io::ErrorKind::PermissionDenied)
    })?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ReferenceTreeError::CannotSocket(
            std::io::ErrorKind::InvalidInput,
        ))?;
    match nix::sys::stat::fstatat(&parent_dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat)
            if nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
                .contains(nix::sys::stat::SFlag::S_IFSOCK) =>
        {
            return set_reference_socket_permissions(&parent_dir, name);
        }
        Ok(stat)
            if nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
                .contains(nix::sys::stat::SFlag::S_IFLNK) =>
        {
            match nix::sys::stat::fstatat(&parent_dir, name, nix::fcntl::AtFlags::empty()) {
                Err(nix::errno::Errno::ENOENT) => {
                    nix::unistd::unlinkat(
                        &parent_dir,
                        name,
                        nix::unistd::UnlinkatFlags::NoRemoveDir,
                    )
                    .map_err(|error| {
                        ReferenceTreeError::CannotSocket(std::io::Error::from(error).kind())
                    })?;
                }
                Ok(_target) => {
                    return Err(ReferenceTreeError::CannotSocket(
                        std::io::ErrorKind::AlreadyExists,
                    ));
                }
                Err(_error) => {
                    return Err(ReferenceTreeError::CannotSocket(
                        std::io::ErrorKind::AlreadyExists,
                    ));
                }
            }
        }
        Ok(_stat) => {
            return Err(ReferenceTreeError::CannotSocket(
                std::io::ErrorKind::AlreadyExists,
            ));
        }
        Err(nix::errno::Errno::ENOENT) => {}
        Err(error) => {
            return Err(ReferenceTreeError::CannotSocket(
                std::io::Error::from(error).kind(),
            ));
        }
    }
    UnixListener::bind(path).map_err(|error| ReferenceTreeError::CannotSocket(error.kind()))?;
    set_reference_socket_permissions(&parent_dir, name)
}

fn set_reference_socket_permissions(
    parent_dir: &fs::File,
    name: &str,
) -> Result<(), ReferenceTreeError> {
    nix::sys::stat::fchmodat(
        parent_dir,
        name,
        nix::sys::stat::Mode::from_bits_truncate(0o777),
        nix::sys::stat::FchmodatFlags::NoFollowSymlink,
    )
    .map_err(|error| ReferenceTreeError::CannotSocket(std::io::Error::from(error).kind()))
}

fn ensure_reference_model_alias(path: &Path, target: &Path) -> Result<(), ReferenceTreeError> {
    if let Ok(existing) = read_reference_symlink(path) {
        if existing == target || is_valid_ctx_model_symlink(&existing) {
            return Ok(());
        }
        if is_legacy_ctx_model_symlink(&existing) {
            remove_reference_entry(path).map_err(|_error| ReferenceTreeError::CannotLink)?;
        } else {
            return Err(ReferenceTreeError::CannotLink);
        }
    } else if path.symlink_metadata().is_ok() {
        return Err(ReferenceTreeError::CannotLink);
    }
    if let Some(parent) = path.parent() {
        create_reference_dir(parent)?;
        let parent_dir = open_reference_dir(parent)?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(ReferenceTreeError::CannotLink)?;
        nix::unistd::symlinkat(target, &parent_dir, name)
            .map_err(|_error| ReferenceTreeError::CannotLink)?;
        return parent_dir
            .sync_all()
            .map_err(|_error| ReferenceTreeError::CannotLink);
    }
    Err(ReferenceTreeError::CannotLink)
}

fn is_valid_ctx_model_symlink(target: &Path) -> bool {
    let Some(target) = target.to_str() else {
        return false;
    };
    let Some(model) = target.strip_prefix("/ctx/model/") else {
        return false;
    };
    abi_path::is_model_reference(model)
}

fn is_legacy_ctx_model_symlink(target: &Path) -> bool {
    let Some(target) = target.to_str() else {
        return false;
    };
    let Some(model) = target.strip_prefix("/ctx/model/") else {
        return false;
    };
    is_object_name(model)
}
