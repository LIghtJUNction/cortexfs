fn push_str_byte_limit(output: &mut String, value: &str, max_bytes: usize) {
    if value.len() <= max_bytes {
        output.push_str(value);
        return;
    }
    let mut end = 0;
    for (index, character) in value.char_indices() {
        let next = index + character.len_utf8();
        if next > max_bytes {
            break;
        }
        end = next;
    }
    if let Some(prefix) = value.get(..end) {
        output.push_str(prefix);
    }
}

fn read_bounded_regular_utf8(path: &Path, max_bytes: u64) -> Option<String> {
    let mut content = String::new();
    let file = open_plain_file_no_follow(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > max_bytes {
        return None;
    }
    file.take(max_bytes.saturating_add(1))
        .read_to_string(&mut content)
        .ok()?;
    if u64::try_from(content.len()).ok()? > max_bytes {
        return None;
    }
    Some(content)
}

fn open_plain_file_no_follow(path: &Path) -> std::io::Result<File> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid file name")
        })?;
    let parent_dir = open_directory_no_symlink_components(parent)?;
    let file = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map_err(std::io::Error::from)?;
    Ok(File::from(file))
}

fn open_directory_no_symlink_components(path: &Path) -> std::io::Result<File> {
    let mut directory = if path.is_absolute() {
        open_directory_no_follow(Path::new("/"))?
    } else {
        open_directory_no_follow(Path::new("."))?
    };
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid directory name")
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
                .map_err(std::io::Error::from)?;
                directory = File::from(next);
            }
            Component::ParentDir | Component::Prefix(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_directory_no_follow(path: &Path) -> std::io::Result<File> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ));
    }
    Ok(directory)
}

fn fd_entry_is_regular_file(parent_dir: &File, name: &str) -> bool {
    nix::sys::stat::fstatat(parent_dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .is_ok_and(|stat| stat.st_mode & libc::S_IFMT == libc::S_IFREG)
}

fn fd_entry_is_directory(parent_dir: &File, name: &str) -> bool {
    nix::sys::stat::fstatat(parent_dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .is_ok_and(|stat| stat.st_mode & libc::S_IFMT == libc::S_IFDIR)
}

fn proc_fd_path(directory: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()))
}

fn read_history_messages_tail(path: &Path) -> std::io::Result<String> {
    let mut file = open_plain_file_no_follow(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "history messages path is not a plain file",
        ));
    }
    let len = metadata.len();
    let read_len = len.min(MAX_HISTORY_MESSAGES_READ_BYTES);
    let start = len.saturating_sub(read_len);
    file.seek(SeekFrom::Start(start))?;

    let read_len_usize = usize::try_from(read_len)
        .map_err(|_error| std::io::Error::other("history tail too large"))?;
    let mut bytes = vec![0; read_len_usize];
    file.read_exact(&mut bytes)?;
    if start > 0
        && let Some(first_newline) = bytes.iter().position(|byte| *byte == b'\n')
    {
        bytes.drain(..=first_newline);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}
