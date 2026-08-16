use crate::*;

pub(crate) fn create_agent_terminal_runtime_dir(path: &Path) -> io::Result<()> {
    create_plain_directory(
        path,
        0o700,
        "agent terminal runtime path is not a plain directory",
        "agent terminal runtime path contains a non-directory entry",
        "invalid agent terminal runtime directory name",
    )
}

pub(crate) fn write_empty_shell_startup_stub(parent: &Path) -> io::Result<()> {
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

pub(crate) fn wait_for_agent_terminal_socket(socket: &Path) -> Result<(), CliError> {
    cortexfs::agent::launch::wait_socket(socket, 50, Duration::from_millis(100)).map_err(|_error| {
        CliError::unavailable(format!(
            "agent terminal service started, but socket did not appear: {}",
            socket.display()
        ))
    })
}

pub(crate) fn agent_runtime_socket(
    root: &Path,
    name: &str,
    session: &str,
) -> Result<PathBuf, CliError> {
    let runtime_root = if let Some(path) = env::var_os("XDG_RUNTIME_DIR") {
        PathBuf::from(path)
    } else {
        let uid = current_uid_for_ctx(root)?
            .parse::<u32>()
            .map_err(|error| CliError::unavailable(format!("invalid current uid: {error}")))?;
        cortexfs_paths::user_runtime_root(uid)
    };
    Ok(cortexfs_paths::terminal_runtime_socket(
        &runtime_root,
        name,
        session,
    ))
}

pub(crate) fn ensure_best_effort_visible_terminal_socket(
    visible_socket: &Path,
    runtime_socket: &Path,
) -> Result<bool, CliError> {
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
        Ok(target) if target == runtime_socket => {
            verify_visible_socket_alias(visible_socket, runtime_socket).map(|()| false)
        }
        Ok(_target) => Err(CliError::unavailable(format!(
            "{} already points at another socket",
            visible_socket.display()
        ))),
        Err(nix::errno::Errno::ENOENT) => {
            match nix::unistd::symlinkat(runtime_socket, &parent_dir, file_name) {
                Ok(()) => match verify_visible_socket_alias(visible_socket, runtime_socket) {
                    Ok(()) => Ok(true),
                    Err(error) => {
                        let _ignored = nix::unistd::unlinkat(
                            &parent_dir,
                            file_name,
                            nix::unistd::UnlinkatFlags::NoRemoveDir,
                        );
                        Err(error)
                    }
                },
                Err(error) => Err(CliError::unavailable(format!(
                    "cannot create terminal socket link {} -> {}: {error}",
                    visible_socket.display(),
                    runtime_socket.display()
                ))),
            }
        }
        Err(error) => Err(CliError::unavailable(format!(
            "cannot inspect {}: {error}",
            visible_socket.display()
        ))),
    }
}

pub(crate) fn verify_visible_socket_alias(
    visible_socket: &Path,
    runtime_socket: &Path,
) -> Result<(), CliError> {
    let parent = visible_socket
        .parent()
        .ok_or_else(|| CliError::unavailable("socket alias path has no parent"))?;
    let parent_dir = open_plain_directory(parent).map_err(|error| {
        CliError::unavailable(format!("cannot open {}: {error}", parent.display()))
    })?;
    let file_name = visible_socket
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::unavailable("invalid socket alias name"))?;
    let target = nix::fcntl::readlinkat(&parent_dir, file_name).map_err(|error| {
        CliError::unavailable(format!(
            "cannot inspect {}: {error}",
            visible_socket.display()
        ))
    })?;
    if Path::new(&target) != runtime_socket {
        return Err(CliError::unavailable(format!(
            "{} does not point at {}",
            visible_socket.display(),
            runtime_socket.display()
        )));
    }
    Ok(())
}

pub(crate) fn current_uid_for_ctx(root: &Path) -> Result<String, CliError> {
    let home = ctx_home(root)?;
    home.file_name()
        .and_then(|uid| uid.to_str())
        .filter(|uid| uid.bytes().all(|byte| byte.is_ascii_digit()))
        .map(str::to_owned)
        .ok_or_else(|| CliError::unavailable("cannot derive uid from CTX_HOME"))
}

pub(crate) fn socket_runtime_dir(socket: &Path) -> Option<PathBuf> {
    socket_bind_path(socket).parent().map(Path::to_path_buf)
}

pub(crate) fn socket_bind_path(socket: &Path) -> PathBuf {
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

pub(crate) fn shell_startup_stub_path(socket: &Path) -> Option<PathBuf> {
    socket_runtime_dir(socket).map(|directory| directory.join(".empty-shell-startup"))
}

pub(crate) fn agent_terminal_unit(name: &str, session: &str) -> String {
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
