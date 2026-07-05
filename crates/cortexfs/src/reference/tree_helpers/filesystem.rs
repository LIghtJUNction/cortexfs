fn create_reference_dir(path: &Path) -> Result<(), ReferenceTreeError> {
    plain_fs::create_plain_dir(path).map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn open_reference_dir(path: &Path) -> Result<fs::File, ReferenceTreeError> {
    plain_fs::open_plain_directory(path).map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn ensure_reference_home_ownership(path: &Path) -> Result<(), ReferenceTreeError> {
    if !nix::unistd::Uid::effective().is_root() {
        return Ok(());
    }
    chown_reference_home_tree(path)
}

fn ensure_reference_agent_control_ownership(path: &Path) -> Result<(), ReferenceTreeError> {
    if !nix::unistd::Uid::effective().is_root() {
        return Ok(());
    }
    let uid = read_reference_owner_id(&path.join("uid"))?;
    let gid = read_reference_owner_id(&path.join("gid"))?;
    chown_reference_tree(path, uid, gid)
}

fn read_reference_owner_id(path: &Path) -> Result<u32, ReferenceTreeError> {
    let value = plain_fs::read_small_text_file(path, 64)
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    value
        .trim()
        .parse::<u32>()
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn chown_reference_home_tree(path: &Path) -> Result<(), ReferenceTreeError> {
    chown_reference_tree(path, REFERENCE_HOME_UID, REFERENCE_HOME_GID)
}

fn chown_reference_tree(path: &Path, uid: u32, gid: u32) -> Result<(), ReferenceTreeError> {
    let metadata = fs::symlink_metadata(path).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    chown_reference_entry(path, uid, gid)?;
    if metadata.is_dir() {
        for entry in fs::read_dir(path).map_err(|_error| ReferenceTreeError::CannotCreate)? {
            let entry = entry.map_err(|_error| ReferenceTreeError::CannotCreate)?;
            chown_reference_tree(&entry.path(), uid, gid)?;
        }
    }
    Ok(())
}

fn chown_reference_entry(path: &Path, uid: u32, gid: u32) -> Result<(), ReferenceTreeError> {
    nix::unistd::fchownat(
        nix::fcntl::AT_FDCWD,
        path,
        Some(nix::unistd::Uid::from_raw(uid)),
        Some(nix::unistd::Gid::from_raw(gid)),
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
                Ok(target)
                    if nix::sys::stat::SFlag::from_bits_truncate(target.st_mode)
                        .contains(nix::sys::stat::SFlag::S_IFSOCK) =>
                {
                    return Ok(());
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
    abi::path::is_model_reference(model)
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
