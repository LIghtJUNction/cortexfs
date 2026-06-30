impl Tool for FsReadTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.read",
            description: "Read a UTF-8 text file from the visible filesystem.",
            input_schema: FS_READ_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let path = invocation
            .string_field("path")
            .unwrap_or_else(|| invocation.input().trim().to_owned());
        if path.is_empty() {
            return Err(ToolError::invalid("missing path"));
        }
        match read_regular_utf8_file(Path::new(&path), MAX_FS_READ_BYTES) {
            Ok(content) => output
                .message(&content)
                .map_err(|error| ToolError::new("EIO", error.to_string())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(ToolError::not_found("file not found"))
            }
            Err(_error) => Err(ToolError::denied("read failed")),
        }
    }
}

impl Tool for FsWriteTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "fs.write",
            description: "Write UTF-8 text to a path in the visible filesystem.",
            input_schema: FS_WRITE_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let path = invocation.string_field("path").unwrap_or_default();
        let content = invocation.string_field("content").unwrap_or_default();
        if path.is_empty() {
            return Err(ToolError::invalid("missing path"));
        }
        write_text_file_atomic(Path::new(&path), &content)
            .map_err(|_error| ToolError::denied("write failed"))?;
        output
            .message("written")
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}

fn run_fs_read_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
    let Some(path) = args.first() else {
        writeln!(io::stderr(), "fs.read: missing path")?;
        return Ok(ExitCode::from(2));
    };
    let content = read_regular_utf8_file(&PathBuf::from(path), MAX_FS_READ_BYTES)?;
    writer.write_all(content.as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

fn read_regular_utf8_file(path: &Path, max_bytes: u64) -> io::Result<String> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path must have a parent directory",
        ));
    };
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))?;
    let parent_dir = open_plain_directory(parent)?;
    let fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_RDONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )?;
    let mut file = fs::File::from(fd);
    let metadata = file.metadata()?;
    let len = regular_file_len(&metadata, max_bytes)?;
    let mut content = vec![0; len];
    file.read_exact(&mut content)?;
    String::from_utf8(content)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.utf8_error()))
}

fn regular_file_len(metadata: &fs::Metadata, max_bytes: u64) -> io::Result<usize> {
    if !metadata.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "path is not a regular file",
        ));
    }
    if metadata.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "file exceeds read limit",
        ));
    }
    usize::try_from(metadata.len()).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("file is too large to read: {error}"),
        )
    })
}

fn run_fs_write_cli(args: &[OsString], writer: &mut dyn Write) -> io::Result<ExitCode> {
    let Some(path) = args.first() else {
        writeln!(io::stderr(), "fs.write: missing path")?;
        return Ok(ExitCode::from(2));
    };
    let content = if args.len() > 1 {
        args.iter()
            .skip(1)
            .map(|value| value.to_string_lossy())
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        read_text_from_stdin_limited(io::stdin(), MAX_FUSE_V1_SMALL_WRITE_BYTES)?
    };
    write_text_file_atomic(Path::new(path), &content)?;
    writeln!(writer, "written")?;
    Ok(ExitCode::SUCCESS)
}

fn read_text_from_stdin_limited(reader: impl Read, max_bytes: usize) -> io::Result<String> {
    let limit = u64::try_from(max_bytes.saturating_add(1)).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("stdin read limit is invalid: {error}"),
        )
    })?;
    let mut content = String::new();
    reader.take(limit).read_to_string(&mut content)?;
    if content.len() > max_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "stdin exceeds fs.write input limit",
        ));
    }
    Ok(content)
}

fn write_text_file_atomic(path: &Path, content: &str) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path must have a parent directory",
        ));
    };
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid file name"))?;
    let parent_dir = open_plain_directory(parent)?;
    for attempt in 0..16 {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let tmp_name = format!(".{file_name}.tmp-{}-{nonce}-{attempt}", std::process::id());
        match nix::fcntl::openat(
            &parent_dir,
            tmp_name.as_str(),
            nix::fcntl::OFlag::O_CREAT
                | nix::fcntl::OFlag::O_EXCL
                | nix::fcntl::OFlag::O_WRONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::from_bits_truncate(0o644),
        ) {
            Ok(fd) => {
                let mut file = fs::File::from(fd);
                if let Err(error) = file.write_all(content.as_bytes()) {
                    let _ignored = nix::unistd::unlinkat(
                        &parent_dir,
                        tmp_name.as_str(),
                        nix::unistd::UnlinkatFlags::NoRemoveDir,
                    );
                    return Err(error);
                }
                if let Err(error) = file.sync_all() {
                    let _ignored = nix::unistd::unlinkat(
                        &parent_dir,
                        tmp_name.as_str(),
                        nix::unistd::UnlinkatFlags::NoRemoveDir,
                    );
                    return Err(error);
                }
                drop(file);
                if let Err(error) =
                    nix::fcntl::renameat(&parent_dir, tmp_name.as_str(), &parent_dir, file_name)
                {
                    let _ignored = nix::unistd::unlinkat(
                        &parent_dir,
                        tmp_name.as_str(),
                        nix::unistd::UnlinkatFlags::NoRemoveDir,
                    );
                    return Err(io::Error::from(error));
                }
                return parent_dir.sync_all();
            }
            Err(nix::errno::Errno::EEXIST) => {}
            Err(error) => return Err(io::Error::from(error)),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "cannot create unique temp file",
    ))
}

fn open_plain_directory(path: &Path) -> io::Result<fs::File> {
    let mut directory = if path.is_absolute() {
        open_single_plain_directory(Path::new("/"))?
    } else {
        open_single_plain_directory(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid directory name")
                })?;
                let next = nix::fcntl::openat(
                    &directory,
                    name,
                    nix::fcntl::OFlag::O_DIRECTORY
                        | nix::fcntl::OFlag::O_RDONLY
                        | nix::fcntl::OFlag::O_NOFOLLOW
                        | nix::fcntl::OFlag::O_CLOEXEC,
                    nix::sys::stat::Mode::empty(),
                )?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "directory path contains unsupported components",
                ));
            }
        }
    }
    Ok(directory)
}

fn open_single_plain_directory(path: &Path) -> io::Result<fs::File> {
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW)
        .open(path)?;
    if !directory.metadata()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a plain directory",
        ));
    }
    Ok(directory)
}
