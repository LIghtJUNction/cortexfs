use super::*;

pub(crate) fn create_reference_dir(path: &Path) -> Result<(), ReferenceTreeError> {
    plain_fs::create_plain_dir(path).map_err(|_error| ReferenceTreeError::CannotCreate)
}

pub(crate) fn open_reference_dir(path: &Path) -> Result<fs::File, ReferenceTreeError> {
    plain_fs::open_plain_directory(path).map_err(|_error| ReferenceTreeError::CannotCreate)
}

pub(crate) fn ensure_reference_home_entry_ownership(path: &Path) -> Result<(), ReferenceTreeError> {
    if !nix::unistd::Uid::effective().is_root() {
        return Ok(());
    }
    chown_reference_home_entry(path, REFERENCE_HOME_UID, REFERENCE_HOME_GID)
}

pub(crate) fn ensure_reference_agent_control_ownership(
    path: &Path,
) -> Result<(), ReferenceTreeError> {
    if !nix::unistd::Uid::effective().is_root() {
        return Ok(());
    }
    let uid = read_reference_owner_id(&path.join("uid"))?;
    let gid = read_reference_owner_id(&path.join("gid"))?;
    chown_reference_tree(path, uid, gid)
}

pub(crate) fn read_reference_owner_id(path: &Path) -> Result<u32, ReferenceTreeError> {
    let value = plain_fs::read_small_text_file(path, 64)
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    value
        .trim()
        .parse::<u32>()
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}

pub(crate) fn chown_reference_tree(
    path: &Path,
    uid: u32,
    gid: u32,
) -> Result<(), ReferenceTreeError> {
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

pub(crate) fn chown_reference_entry(
    path: &Path,
    uid: u32,
    gid: u32,
) -> Result<(), ReferenceTreeError> {
    nix::unistd::fchownat(
        nix::fcntl::AT_FDCWD,
        path,
        Some(nix::unistd::Uid::from_raw(uid)),
        Some(nix::unistd::Gid::from_raw(gid)),
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| ReferenceTreeError::CannotCreate)
}

pub(crate) fn chown_reference_home_entry(
    path: &Path,
    uid: u32,
    gid: u32,
) -> Result<(), ReferenceTreeError> {
    let metadata = fs::symlink_metadata(path).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    if metadata.is_dir() {
        let directory = open_reference_dir(path)?;
        chown_reference_open_entry(&directory, uid, gid)?;
        return chown_reference_directory_symlinks(&directory, uid, gid);
    }
    if !metadata.is_file() {
        return Err(ReferenceTreeError::CannotCreate);
    }
    let file =
        plain_fs::open_plain_file(path).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    if !file
        .metadata()
        .map_err(|_error| ReferenceTreeError::CannotCreate)?
        .is_file()
    {
        return Err(ReferenceTreeError::CannotCreate);
    }
    chown_reference_open_entry(&file, uid, gid)
}

fn chown_reference_directory_symlinks(
    directory: &fs::File,
    uid: u32,
    gid: u32,
) -> Result<(), ReferenceTreeError> {
    for entry in fs::read_dir(plain_fs::proc_fd_path(directory))
        .map_err(|_error| ReferenceTreeError::CannotCreate)?
    {
        let entry = entry.map_err(|_error| ReferenceTreeError::CannotCreate)?;
        let name = reference_tree_entry_name(&entry)?;
        let stat = nix::sys::stat::fstatat(
            directory,
            name.as_str(),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
        if nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
            .contains(nix::sys::stat::SFlag::S_IFLNK)
        {
            nix::unistd::fchownat(
                directory,
                name.as_str(),
                Some(nix::unistd::Uid::from_raw(uid)),
                Some(nix::unistd::Gid::from_raw(gid)),
                nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
            )
            .map_err(|_error| ReferenceTreeError::CannotCreate)?;
        }
    }
    Ok(())
}

fn chown_reference_open_entry(
    file: &fs::File,
    uid: u32,
    gid: u32,
) -> Result<(), ReferenceTreeError> {
    nix::unistd::fchown(
        file,
        Some(nix::unistd::Uid::from_raw(uid)),
        Some(nix::unistd::Gid::from_raw(gid)),
    )
    .map_err(|_error| ReferenceTreeError::CannotCreate)
}

pub(crate) fn write_reference_text(path: &Path, content: &str) -> Result<(), ReferenceTreeError> {
    if let Some(parent) = path.parent() {
        create_reference_dir(parent)?;
    }
    atomic_replace_text_with_mode(path, content, 0o644)
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}

pub(crate) fn set_reference_executable(path: &Path) -> Result<(), ReferenceTreeError> {
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

pub(crate) fn ensure_reference_socket(
    path: &Path,
    uid: u32,
    gid: u32,
) -> Result<(), ReferenceTreeError> {
    plain_fs::ensure_socket_placeholder(path, 0o777)
        .map_err(|error| ReferenceTreeError::CannotSocket(error.kind()))?;
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| ReferenceTreeError::CannotSocket(error.kind()))?;
    if metadata.file_type().is_symlink() {
        return Ok(());
    }
    if !metadata.file_type().is_socket() {
        return Err(ReferenceTreeError::CannotSocket(
            std::io::ErrorKind::AlreadyExists,
        ));
    }
    if should_repair_reference_owner(nix::unistd::Uid::effective().as_raw()) {
        chown_reference_entry(path, uid, gid)?;
    }
    Ok(())
}

pub(crate) const fn should_repair_reference_owner(effective_uid: u32) -> bool {
    effective_uid == 0
}

pub(crate) fn ensure_reference_model_alias(
    path: &Path,
    target: &Path,
) -> Result<(), ReferenceTreeError> {
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

pub(crate) fn is_valid_ctx_model_symlink(target: &Path) -> bool {
    let Some(target) = target.to_str() else {
        return false;
    };
    let Some(model) = target.strip_prefix("/ctx/model/") else {
        return false;
    };
    abi::path::is_model_reference(model)
}

pub(crate) fn is_legacy_ctx_model_symlink(target: &Path) -> bool {
    let Some(target) = target.to_str() else {
        return false;
    };
    let Some(model) = target.strip_prefix("/ctx/model/") else {
        return false;
    };
    is_object_name(model)
}
