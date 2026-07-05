fn require_shared_queue_directory(
    path: &Path,
    label: &str,
    issues: &mut Vec<SharedQueueLayoutIssue>,
) {
    match path.symlink_metadata() {
        Ok(metadata) if metadata.file_type().is_dir() => {}
        Ok(_metadata) => issues.push(SharedQueueLayoutIssue::NotDirectory(label.to_owned())),
        Err(_error) => issues.push(SharedQueueLayoutIssue::MissingDirectory(label.to_owned())),
    }
}

fn queue_child_dir(queue_dir: &Path, name: &str) -> io::Result<PathBuf> {
    let path = queue_dir.join(name);
    crate::plain_fs::open_plain_directory(&path)?;
    Ok(path)
}

fn queue_child_dir_fd(queue_dir: &Path, name: &str) -> io::Result<fs::File> {
    crate::plain_fs::open_plain_directory(&queue_dir.join(name))
}

fn queue_job_plain_dir_fd(queue_dir: &Path, parent: &str, job_name: &str) -> io::Result<fs::File> {
    let parent_dir = queue_child_dir_fd(queue_dir, parent)?;
    open_queue_entry_dir(&parent_dir, job_name)
}

fn open_queue_entry_dir(parent_dir: &fs::File, name: &str) -> io::Result<fs::File> {
    let directory = nix::fcntl::openat(
        parent_dir,
        name,
        nix::fcntl::OFlag::O_DIRECTORY
            | nix::fcntl::OFlag::O_RDONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(io::Error::from)?;
    Ok(fs::File::from(directory))
}

fn fd_entry_is_plain_file(parent_dir: &fs::File, name: &str) -> bool {
    nix::sys::stat::fstatat(parent_dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .is_ok_and(|stat| is_regular_mode(stat.st_mode))
}

fn fd_entry_exists(parent_dir: &fs::File, name: &str) -> bool {
    nix::sys::stat::fstatat(parent_dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW).is_ok()
}

fn path_exists_no_follow(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

fn is_regular_mode(mode: nix::libc::mode_t) -> bool {
    mode & nix::libc::S_IFMT == nix::libc::S_IFREG
}

fn is_queue_job_name(name: &str) -> bool {
    is_object_name(name) && name.ends_with(".req.json")
}

fn write_queue_result_atomic(
    output_dir: &Path,
    output_dir_fd: &fs::File,
    job_name: &str,
    result_name: &str,
    result: &[u8],
) -> io::Result<()> {
    for attempt in 0..16 {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let temp_name = format!(
            ".{job_name}.result.tmp-{}-{nonce}-{attempt}",
            std::process::id()
        );
        match write_new_file_synced_at(output_dir_fd, &temp_name, result) {
            Ok(()) => {
                if let Err(error) = nix::unistd::linkat(
                    output_dir_fd,
                    temp_name.as_str(),
                    output_dir_fd,
                    result_name,
                    nix::fcntl::AtFlags::empty(),
                ) {
                    let _ignored = nix::unistd::unlinkat(
                        output_dir_fd,
                        temp_name.as_str(),
                        nix::unistd::UnlinkatFlags::NoRemoveDir,
                    );
                    return Err(io::Error::from(error));
                }
                if let Err(error) = nix::unistd::unlinkat(
                    output_dir_fd,
                    temp_name.as_str(),
                    nix::unistd::UnlinkatFlags::NoRemoveDir,
                ) {
                    let _ignored = nix::unistd::unlinkat(
                        output_dir_fd,
                        result_name,
                        nix::unistd::UnlinkatFlags::NoRemoveDir,
                    );
                    return Err(io::Error::from(error));
                }
                if let Err(error) = output_dir_fd.sync_all() {
                    let _ignored = nix::unistd::unlinkat(
                        output_dir_fd,
                        result_name,
                        nix::unistd::UnlinkatFlags::NoRemoveDir,
                    );
                    return Err(error);
                }
                if let Err(error) = crate::plain_fs::sync_plain_dir(output_dir) {
                    let _ignored = nix::unistd::unlinkat(
                        output_dir_fd,
                        result_name,
                        nix::unistd::UnlinkatFlags::NoRemoveDir,
                    );
                    return Err(error);
                }
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot create unique queue result temp file",
    ))
}

fn write_new_file_synced_at(parent_dir: &fs::File, name: &str, content: &[u8]) -> io::Result<()> {
    let file_fd = nix::fcntl::openat(
        parent_dir,
        name,
        nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_EXCL
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::from_bits_truncate(0o644),
    )
    .map_err(io::Error::from)?;
    let mut file = fs::File::from(file_fd);
    if let Err(error) = file.set_permissions(fs::Permissions::from_mode(0o644)) {
        let _ignored =
            nix::unistd::unlinkat(parent_dir, name, nix::unistd::UnlinkatFlags::NoRemoveDir);
        return Err(error);
    }
    if let Err(error) = file.write_all(content) {
        let _ignored =
            nix::unistd::unlinkat(parent_dir, name, nix::unistd::UnlinkatFlags::NoRemoveDir);
        return Err(error);
    }
    if let Err(error) = file.sync_all() {
        let _ignored =
            nix::unistd::unlinkat(parent_dir, name, nix::unistd::UnlinkatFlags::NoRemoveDir);
        return Err(error);
    }
    Ok(())
}
