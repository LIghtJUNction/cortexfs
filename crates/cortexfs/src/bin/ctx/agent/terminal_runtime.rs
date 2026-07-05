fn create_agent_terminal_runtime_dir(path: &Path) -> io::Result<()> {
    create_plain_directory(
        path,
        0o700,
        "agent terminal runtime path is not a plain directory",
        "agent terminal runtime path contains a non-directory entry",
        "invalid agent terminal runtime directory name",
    )
}

fn write_empty_shell_startup_stub(parent: &Path) -> io::Result<()> {
    let path = parent.join(".empty-shell-startup");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&path)?;
    file.write_all(b"")?;
    file.sync_all()?;
    open_plain_directory(parent)?.sync_all()
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
    if let Err(error) = create_plain_directory(
        parent,
        0o755,
        "agent terminal runtime path is not a plain directory",
        "agent terminal runtime path contains a non-directory entry",
        "invalid agent terminal runtime directory name",
    ) {
        if visible_terminal_write_error_is_best_effort(&error) {
            return Ok(());
        }
        return Err(CliError::unavailable(format!(
            "cannot create {}: {error}",
            parent.display()
        )));
    }
    let parent_dir = open_plain_directory(parent).map_err(|error| {
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
            if code == libc::EACCES
                || code == libc::EPERM
                || code == libc::ENOSYS
                || code == libc::EROFS
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
    let Ok(parent_dir) = open_plain_directory(parent) else {
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
