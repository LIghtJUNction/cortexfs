use crate::*;

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
