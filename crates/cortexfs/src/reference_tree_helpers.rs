const REFERENCE_HOME_UID: u32 = 1000;
const REFERENCE_HOME_GID: u32 = 1000;
const MAX_REFERENCE_SESSION_META_BYTES: u64 = 64 * 1024;

fn ensure_reference_home(root: &Path) -> Result<(), ReferenceTreeError> {
    for agent in ["base", "coder", "reviewer", "executor"] {
        ensure_reference_home_agent(root, agent)?;
    }
    create_reference_dir(&root.join("home").join("1000").join("tool"))?;
    create_reference_dir(&root.join("home").join("1000").join("model"))?;
    write_reference_text(
        &root.join("home").join("1000").join(".tshrc"),
        "CTX_PATH=/ctx/tool:/ctx/home/1000/tool\n",
    )?;

    ensure_reference_model_alias(
        &root.join("home").join("1000").join("model").join("coder"),
        Path::new("/ctx/model/main"),
    )?;
    ensure_reference_home_ownership(&root.join("home").join("1000"))
}

fn ensure_reference_home_agent(root: &Path, agent: &str) -> Result<(), ReferenceTreeError> {
    let agent_root = root.join("home").join("1000").join("agent").join(agent);
    create_reference_dir(&agent_root.join("root"))?;
    create_reference_dir(&agent_root.join("session").join("index").join("by-cwd"))?;
    create_reference_dir(&agent_root.join("session").join("index").join("by-hash"))?;
    create_reference_dir(&agent_root.join("session").join("index").join("by-uuid"))?;
    create_reference_dir(&agent_root.join("data"))?;
    create_reference_dir(&agent_root.join("cache"))?;
    create_reference_dir(&agent_root.join("log"))
}

fn remove_deprecated_reference_home_tool_aliases(root: &Path) -> Result<(), ReferenceTreeError> {
    let alias = root.join("home").join("1000").join("tool").join("fs.read");
    match read_reference_symlink(&alias) {
        Ok(target) if target == Path::new("/ctx/tool/fs.read") => remove_reference_entry(&alias),
        Ok(_) | Err(_) => Ok(()),
    }
}

fn migrate_reference_legacy_session_meta_models(root: &Path) -> Result<(), ReferenceTreeError> {
    let mut meta_paths = Vec::new();
    collect_reference_agent_session_meta_paths(&root.join("home"), &mut meta_paths)?;
    collect_reference_shared_agent_session_meta_paths(&root.join("shared"), &mut meta_paths)?;
    for meta_path in meta_paths {
        migrate_reference_session_meta_model(&meta_path)?;
    }
    Ok(())
}

fn collect_reference_agent_session_meta_paths(
    home_root: &Path,
    meta_paths: &mut Vec<PathBuf>,
) -> Result<(), ReferenceTreeError> {
    let Ok(home_dir) = open_reference_dir(home_root) else {
        return Ok(());
    };
    let users = reference_tree_read_dir(&home_dir)?;
    for user in users {
        let user = user.map_err(|_error| ReferenceTreeError::CannotCreate)?;
        let user_name = reference_tree_entry_name(&user)?;
        if reference_tree_entry_is_directory(&home_dir, user_name.as_str())? {
            collect_reference_session_meta_paths(
                &home_root.join(&user_name).join("agent"),
                meta_paths,
            )?;
        }
    }
    Ok(())
}

fn collect_reference_shared_agent_session_meta_paths(
    shared_root: &Path,
    meta_paths: &mut Vec<PathBuf>,
) -> Result<(), ReferenceTreeError> {
    let Ok(shared_dir) = open_reference_dir(shared_root) else {
        return Ok(());
    };
    let spaces = reference_tree_read_dir(&shared_dir)?;
    for space in spaces {
        let space = space.map_err(|_error| ReferenceTreeError::CannotCreate)?;
        let space_name = reference_tree_entry_name(&space)?;
        if reference_tree_entry_is_directory(&shared_dir, space_name.as_str())? {
            collect_reference_session_meta_paths(
                &shared_root.join(&space_name).join("agent"),
                meta_paths,
            )?;
        }
    }
    Ok(())
}

fn collect_reference_session_meta_paths(
    agent_root: &Path,
    meta_paths: &mut Vec<PathBuf>,
) -> Result<(), ReferenceTreeError> {
    let Ok(agent_root_dir) = open_reference_dir(agent_root) else {
        return Ok(());
    };
    let agents = reference_tree_read_dir(&agent_root_dir)?;
    for agent in agents {
        let agent = agent.map_err(|_error| ReferenceTreeError::CannotCreate)?;
        let agent_name = reference_tree_entry_name(&agent)?;
        if !reference_tree_entry_is_directory(&agent_root_dir, agent_name.as_str())? {
            continue;
        }
        let session_root = agent_root.join(&agent_name).join("session");
        let Ok(session_root_dir) = open_reference_dir(&session_root) else {
            continue;
        };
        let sessions = reference_tree_read_dir(&session_root_dir)?;
        for session in sessions {
            let session = session.map_err(|_error| ReferenceTreeError::CannotCreate)?;
            let session_name = reference_tree_entry_name(&session)?;
            if !reference_tree_entry_is_directory(&session_root_dir, session_name.as_str())? {
                continue;
            }
            let session_path = session_root.join(&session_name);
            let Ok(session_dir) = open_reference_dir(&session_path) else {
                continue;
            };
            if reference_tree_entry_is_file(&session_dir, "meta.json")? {
                meta_paths.push(session_path.join("meta.json"));
            }
        }
    }
    Ok(())
}

fn reference_tree_read_dir(directory: &fs::File) -> Result<fs::ReadDir, ReferenceTreeError> {
    fs::read_dir(reference_tree_proc_fd_path(directory)).map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn reference_tree_entry_name(entry: &fs::DirEntry) -> Result<String, ReferenceTreeError> {
    entry
        .file_name()
        .to_str()
        .map(str::to_owned)
        .ok_or(ReferenceTreeError::CannotCreate)
}

fn reference_tree_entry_is_directory(
    directory: &fs::File,
    name: &str,
) -> Result<bool, ReferenceTreeError> {
    Ok(reference_tree_entry_file_type(directory, name)? == libc::S_IFDIR)
}

fn reference_tree_entry_is_file(
    directory: &fs::File,
    name: &str,
) -> Result<bool, ReferenceTreeError> {
    let stat = match nix::sys::stat::fstatat(
        directory,
        name,
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(stat) => stat,
        Err(nix::errno::Errno::ENOENT) => return Ok(false),
        Err(_error) => return Err(ReferenceTreeError::CannotCreate),
    };
    Ok(stat.st_mode & libc::S_IFMT == libc::S_IFREG)
}

fn reference_tree_entry_file_type(
    directory: &fs::File,
    name: &str,
) -> Result<libc::mode_t, ReferenceTreeError> {
    let stat = nix::sys::stat::fstatat(
        directory,
        name,
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    Ok(stat.st_mode & libc::S_IFMT)
}

fn read_reference_symlink(path: &Path) -> Result<PathBuf, ReferenceTreeError> {
    let Some(parent) = path.parent() else {
        return Err(ReferenceTreeError::CannotUnlink);
    };
    let directory = open_reference_dir(parent).map_err(|_error| ReferenceTreeError::CannotUnlink)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ReferenceTreeError::CannotUnlink)?;
    nix::fcntl::readlinkat(&directory, name)
        .map(PathBuf::from)
        .map_err(|_error| ReferenceTreeError::CannotUnlink)
}

fn remove_reference_entry(path: &Path) -> Result<(), ReferenceTreeError> {
    let Some(parent) = path.parent() else {
        return Err(ReferenceTreeError::CannotUnlink);
    };
    let directory = open_reference_dir(parent).map_err(|_error| ReferenceTreeError::CannotUnlink)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ReferenceTreeError::CannotUnlink)?;
    nix::unistd::unlinkat(&directory, name, nix::unistd::UnlinkatFlags::NoRemoveDir)
        .map_err(|_error| ReferenceTreeError::CannotUnlink)
}

fn migrate_reference_session_meta_model(meta_path: &Path) -> Result<(), ReferenceTreeError> {
    let content = read_reference_session_meta(meta_path)
        .map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let Ok(mut value) = serde_json::from_str::<Value>(&content) else {
        return Ok(());
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    let Some(model) = object.get("model").and_then(Value::as_str) else {
        return Ok(());
    };
    if abi_path::is_model_reference(model) || !is_object_name(model) {
        return Ok(());
    }

    object.insert("model".to_owned(), serde_json::json!("debug/echo"));
    let content =
        serde_json::to_string(&value).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    atomic_replace_text(meta_path, &format!("{content}\n"))
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn read_reference_session_meta(path: &Path) -> std::io::Result<String> {
    let mut file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_REFERENCE_SESSION_META_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "reference session metadata is too large or not a plain file",
        ));
    }
    let len = usize::try_from(metadata.len())
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.utf8_error()))
}

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
