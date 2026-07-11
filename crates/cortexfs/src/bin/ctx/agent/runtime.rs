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

pub(crate) fn agent_runtime_socket(
    root: &Path,
    name: &str,
    session: &str,
) -> Result<PathBuf, CliError> {
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

pub(crate) fn agent_legacy_runtime_socket(
    root: &Path,
    name: &str,
    session: &str,
) -> Result<PathBuf, CliError> {
    Ok(PathBuf::from("/run")
        .join("cortexfs")
        .join("terminal")
        .join(current_uid_for_ctx(root)?)
        .join(name)
        .join(session)
        .join("main.sock"))
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

pub(crate) fn remove_exact_socket_alias(
    visible_socket: &Path,
    runtime_socket: &Path,
) -> io::Result<bool> {
    let parent = visible_socket
        .parent()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let parent_dir = open_plain_directory(parent)?;
    let name = visible_socket
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    let Some(claim) = claim_socket_entry(&parent_dir, name)? else {
        return Ok(false);
    };
    let validation = (|| {
        let stat = nix::sys::stat::fstatat(
            &parent_dir,
            claim.as_str(),
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(io::Error::from)?;
        if !nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
            .contains(nix::sys::stat::SFlag::S_IFLNK)
        {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "refusing to remove non-alias socket path",
            ));
        }
        let target =
            nix::fcntl::readlinkat(&parent_dir, claim.as_str()).map_err(io::Error::from)?;
        if Path::new(&target) != runtime_socket {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "refusing to remove mismatched socket alias",
            ));
        }
        Ok(())
    })();
    if let Err(error) = validation {
        restore_socket_claim(&parent_dir, &claim, name)?;
        return Err(error);
    }
    nix::unistd::unlinkat(
        &parent_dir,
        claim.as_str(),
        nix::unistd::UnlinkatFlags::NoRemoveDir,
    )
    .map_err(io::Error::from)?;
    parent_dir.sync_all()?;
    Ok(true)
}

pub(crate) fn claim_socket_entry(parent: &fs::File, name: &str) -> io::Result<Option<String>> {
    for attempt in 0..16_u8 {
        let claim = cortexfs::authority::helpers::generated_sibling_name(name, "claim", attempt);
        match nix::fcntl::renameat2(
            parent,
            name,
            parent,
            claim.as_str(),
            nix::fcntl::RenameFlags::RENAME_NOREPLACE,
        ) {
            Ok(()) => return Ok(Some(claim)),
            Err(nix::errno::Errno::ENOENT) => return Ok(None),
            Err(nix::errno::Errno::EEXIST) => {}
            Err(error) => return Err(io::Error::from(error)),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot create unique socket claim",
    ))
}

pub(crate) fn restore_socket_claim(parent: &fs::File, claim: &str, name: &str) -> io::Result<()> {
    nix::fcntl::renameat2(
        parent,
        claim,
        parent,
        name,
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(io::Error::from)
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
