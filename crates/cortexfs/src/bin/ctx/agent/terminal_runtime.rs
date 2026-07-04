fn create_agent_terminal_runtime_dir(path: &Path) -> io::Result<()> {
    create_agent_terminal_plain_dir(path, 0o700)
}

fn create_agent_terminal_plain_dir(path: &Path, mode: u32) -> io::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() {
            sync_agent_terminal_runtime_dir(path)
        } else {
            Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "agent terminal runtime path is not a plain directory",
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
                    "agent terminal runtime path contains a non-directory entry",
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

    let mut parent_dir = if let Some(existing_parent) = missing.last().and_then(|path| path.parent())
    {
        open_agent_terminal_runtime_dir(existing_parent)?
    } else {
        return Ok(());
    };

    for directory in missing.iter().rev() {
        let name = directory.file_name().and_then(|name| name.to_str()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid agent terminal runtime directory name",
            )
        })?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(mode),
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

fn sync_agent_terminal_runtime_dir(path: &Path) -> io::Result<()> {
    let directory = open_agent_terminal_runtime_dir(path)?;
    directory.sync_all()
}

fn open_agent_terminal_runtime_dir(path: &Path) -> io::Result<fs::File> {
    let mut directory = if path.is_absolute() {
        open_single_agent_terminal_runtime_dir(Path::new("/"))?
    } else {
        open_single_agent_terminal_runtime_dir(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "agent terminal runtime path is not utf-8",
                    )
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
                    "agent terminal runtime path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_single_agent_terminal_runtime_dir(path: &Path) -> io::Result<fs::File> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "agent terminal runtime path is not a directory",
        ));
    }
    Ok(directory)
}

fn write_empty_shell_startup_stub(parent: &Path) -> io::Result<()> {
    let path = parent.join(".empty-shell-startup");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(&path)?;
    file.write_all(b"")?;
    file.sync_all()?;
    sync_agent_terminal_runtime_dir(parent)
}

fn remove_stale_agent_terminal_socket(socket: &Path) -> io::Result<()> {
    let parent = socket.parent().unwrap_or_else(|| Path::new("."));
    let parent = open_agent_terminal_runtime_dir(parent)?;
    let file_name = socket
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid terminal socket name"))?;
    match nix::sys::stat::fstatat(
        &parent,
        file_name,
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(stat)
            if nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
                .contains(nix::sys::stat::SFlag::S_IFSOCK) =>
        {
            nix::unistd::unlinkat(&parent, file_name, nix::unistd::UnlinkatFlags::NoRemoveDir)
                .map_err(io::Error::from)
        }
        Ok(_metadata) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "refusing to remove non-socket terminal path",
        )),
        Err(nix::errno::Errno::ENOENT) => Ok(()),
        Err(error) => Err(io::Error::from(error)),
    }
}

fn wait_for_agent_terminal_socket(socket: &Path) -> Result<(), CliError> {
    for _ in 0..50 {
        if terminal_socket_exists(socket) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(CliError::unavailable(format!(
        "agent terminal service started, but socket did not appear: {}",
        socket.display()
    )))
}

fn agent_runtime_socket(root: &Path, name: &str, session: &str) -> Result<PathBuf, CliError> {
    let runtime_root = match env::var_os("XDG_RUNTIME_DIR") {
        Some(path) => PathBuf::from(path),
        None => PathBuf::from("/run")
            .join("user")
            .join(current_uid_for_ctx(root)?),
    };
    Ok(runtime_root
        .join("cortexfs")
        .join("terminal")
        .join(name)
        .join(session)
        .join("main.sock"))
}

fn agent_legacy_runtime_socket(root: &Path, name: &str, session: &str) -> Result<PathBuf, CliError> {
    Ok(PathBuf::from("/run")
        .join("cortexfs")
        .join("terminal")
        .join(current_uid_for_ctx(root)?)
        .join(name)
        .join(session)
        .join("main.sock"))
}

fn ensure_best_effort_visible_terminal_socket(
    visible_socket: &Path,
    runtime_socket: &Path,
) -> Result<(), CliError> {
    let Some(parent) = visible_socket.parent() else {
        return Err(CliError::unavailable("terminal socket path has no parent"));
    };
    if let Err(error) = create_agent_terminal_plain_dir(parent, 0o755) {
        if visible_terminal_write_error_is_best_effort(&error) {
            return Ok(());
        }
        return Err(CliError::unavailable(format!(
            "cannot create {}: {error}",
            parent.display()
        )));
    }
    let parent_dir = open_agent_terminal_runtime_dir(parent).map_err(|error| {
        CliError::unavailable(format!("cannot open {}: {error}", parent.display()))
    })?;
    let file_name = visible_socket
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::unavailable("invalid terminal socket link name"))?;
    match nix::fcntl::readlinkat(&parent_dir, file_name).map(PathBuf::from) {
        Ok(target) if target == runtime_socket => Ok(()),
        Ok(_target) => Err(CliError::unavailable(format!(
            "{} already points at another socket",
            visible_socket.display()
        ))),
        Err(nix::errno::Errno::ENOENT) => {
            match nix::unistd::symlinkat(runtime_socket, &parent_dir, file_name) {
                Ok(()) => Ok(()),
                Err(error) if visible_terminal_errno_is_best_effort(error) => Ok(()),
                Err(error) => Err(CliError::unavailable(format!(
                    "cannot create terminal socket link {} -> {}: {error}",
                    visible_socket.display(),
                    runtime_socket.display()
                ))),
            }
        }
        Err(error) if visible_terminal_errno_is_best_effort(error) => Ok(()),
        Err(error) => Err(CliError::unavailable(format!(
            "cannot inspect {}: {error}",
            visible_socket.display()
        ))),
    }
}

fn visible_terminal_write_error_is_best_effort(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::Unsupported
    ) || matches!(
        error.raw_os_error(),
        Some(code)
            if code == nix::libc::EACCES
                || code == nix::libc::EPERM
                || code == nix::libc::ENOSYS
                || code == nix::libc::EROFS
    )
}

fn visible_terminal_errno_is_best_effort(error: nix::errno::Errno) -> bool {
    matches!(
        error,
        nix::errno::Errno::EACCES
            | nix::errno::Errno::EPERM
            | nix::errno::Errno::ENOSYS
            | nix::errno::Errno::EROFS
    )
}

fn current_uid_for_ctx(root: &Path) -> Result<String, CliError> {
    let home = ctx_home(root)?;
    home.file_name()
        .and_then(|uid| uid.to_str())
        .filter(|uid| uid.bytes().all(|byte| byte.is_ascii_digit()))
        .map(str::to_owned)
        .ok_or_else(|| CliError::unavailable("cannot derive uid from CTX_HOME"))
}

fn socket_runtime_dir(socket: &Path) -> Option<PathBuf> {
    socket_bind_path(socket).parent().map(Path::to_path_buf)
}

fn socket_bind_path(socket: &Path) -> PathBuf {
    let Some(parent) = socket.parent() else {
        return socket.to_path_buf();
    };
    let Ok(parent_dir) = open_agent_terminal_runtime_dir(parent) else {
        return socket.to_path_buf();
    };
    let Some(file_name) = socket.file_name().and_then(|name| name.to_str()) else {
        return socket.to_path_buf();
    };
    match nix::fcntl::readlinkat(&parent_dir, file_name).map(PathBuf::from) {
        Ok(target) if target.is_absolute() => target,
        Ok(target) => parent.join(&target),
        Err(_error) => socket.to_path_buf(),
    }
}

fn shell_startup_stub_path(socket: &Path) -> Option<PathBuf> {
    socket_runtime_dir(socket).map(|directory| directory.join(".empty-shell-startup"))
}

fn agent_terminal_unit(name: &str, session: &str) -> String {
    let session = session
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    format!("cortexfs-agent-{name}-{session}-terminal")
}
