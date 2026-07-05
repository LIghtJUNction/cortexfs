fn append_session_lines(dir: &Path, file: &str, lines: &[&str]) -> SocketRecordResult<()> {
    for line in lines {
        append_jsonl_line(&dir.join(file), line)
            .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    }
    Ok(())
}

fn write_session_file(dir: &Path, file: &str, content: &str) -> SocketRecordResult<()> {
    atomic_replace_text(&dir.join(file), content)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)
}

fn set_session_state(dir: &Path, state: &str) -> SocketRecordResult<()> {
    write_session_file(dir, "state", &format!("{state}\n"))?;
    touch_session(dir)
}

fn touch_session(dir: &Path) -> SocketRecordResult<()> {
    write_session_file(dir, "updated_at", &unix_timestamp_text())
}

fn write_text_file_if_absent(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "text file must have a parent",
        )
    })?;
    let name = plain_fs::plain_file_name(path)?;
    let parent_dir = plain_fs::open_plain_directory(parent)?;
    match nix::fcntl::openat(
        &parent_dir,
        name,
        nix::fcntl::OFlag::O_RDWR | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    ) {
        Ok(file_fd) => {
            let file = fs::File::from(file_fd);
            if !file.metadata()?.is_file() {
                return Err(std::io::Error::other("path is not a regular file"));
            }
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
            file.sync_all()?;
            parent_dir.sync_all()?;
            return Ok(());
        }
        Err(nix::errno::Errno::ENOENT) => {}
        Err(error) => return Err(std::io::Error::from(error)),
    }
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        name,
        nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_EXCL
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    )
    .map_err(std::io::Error::from)?;
    let mut file = fs::File::from(file_fd);
    file.write_all(content.as_bytes())?;
    file.flush()?;
    file.sync_all()?;
    parent_dir.sync_all()?;
    Ok(())
}

fn create_private_context_dir(path: &Path) -> std::io::Result<()> {
    match open_private_context_dir(path) {
        Ok(dir) => {
            dir.set_permissions(fs::Permissions::from_mode(0o700))?;
            dir.sync_all()
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path.parent().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "context directory must have a parent",
                )
            })?;
            let name = plain_fs::plain_file_name(path)?;
            let parent_dir = plain_fs::open_plain_directory(parent)?;
            nix::sys::stat::mkdirat(
                &parent_dir,
                name,
                nix::sys::stat::Mode::from_bits_truncate(0o700),
            )
            .map_err(std::io::Error::from)?;
            parent_dir.sync_all()?;
            let dir = open_private_context_dir(path)?;
            dir.set_permissions(fs::Permissions::from_mode(0o700))?;
            dir.sync_all()?;
            parent_dir.sync_all()?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn open_private_context_dir(path: &Path) -> std::io::Result<fs::File> {
    let dir = plain_fs::open_plain_directory(path)?;
    if !dir.metadata()?.is_dir() {
        return Err(std::io::Error::other("path is not a directory"));
    }
    Ok(dir)
}

fn ensure_trailing_newline(content: &str) -> String {
    if content.is_empty() || content.ends_with('\n') {
        content.to_owned()
    } else {
        format!("{content}\n")
    }
}

fn require_socket_session_name(
    session_dir: &Path,
    session: &str,
) -> Result<(), SocketSessionRecordError> {
    if session_dir.file_name().and_then(|name| name.to_str()) == Some(session) {
        Ok(())
    } else {
        Err(SocketSessionRecordError::SessionMismatch)
    }
}

fn require_socket_session_files(session_dir: &Path) -> Result<(), SocketSessionRecordError> {
    for file in SESSION_REQUIRED_FILES {
        if !is_plain_existing_file(&session_dir.join(file)) {
            return Err(SocketSessionRecordError::MissingSessionFile(file));
        }
    }
    Ok(())
}

fn is_plain_existing_file(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_file())
}

fn is_plain_existing_dir(path: &Path) -> bool {
    path.symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir())
}

fn validate_socket_object_field(
    field: &'static str,
    value: &str,
) -> Result<(), SocketRequestError> {
    if is_object_name(value) {
        Ok(())
    } else {
        Err(SocketRequestError::InvalidField {
            field,
            value: value.to_owned(),
        })
    }
}
