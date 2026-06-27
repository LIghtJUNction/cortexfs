fn exec_object(root: &Path, path: &str, args: &[String]) -> Result<ExitCode, CliError> {
    let abi_path = classify_input_path(root, path)?;
    if !matches!(
        classify_abi_path(&abi_path),
        "ctx.model.exec" | "ctx.agent.exec" | "ctx.tool.exec"
    ) {
        return Err(CliError::usage(format!(
            "exec requires model/NAME, agent/NAME, or tool/NAME: {path}"
        )));
    }

    let path = resolve_abi_path(root, path)?;
    let executable = open_executable_no_follow(&path)?;
    let status = object_execution_command(root, &proc_fd_path(&executable))
        .args(args)
        .status()
        .map_err(|error| {
            CliError::unavailable(format!("cannot exec {}: {error}", path.display()))
        })?;

    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or_else(|| ExitCode::from(70), ExitCode::from))
}

fn object_execution_command(root: &Path, path: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new(path);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("CTX_ROOT", root);
    command
}

fn file_command(root: &Path, args: &FileArgs) -> Result<(), CliError> {
    match args.command {
        FileCommand::Info => file_info(root, &args.path),
        FileCommand::Type => file_type(root, &args.path),
        FileCommand::Check => file_check(root, &args.path),
    }
}

fn file_cat(root: &Path, path: &str) -> Result<(), CliError> {
    let path = resolve_abi_path(root, path)?;
    cat_path(&path)
}

fn cat_path(path: &Path) -> Result<(), CliError> {
    let mut file = open_plain_read_file(path)?;
    let metadata = file.metadata().map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
    })?;
    if !metadata.is_file() {
        return Err(CliError::unavailable(format!(
            "cannot read {}: not a regular file",
            path.display()
        )));
    }
    let mut stdout = io::stdout().lock();
    io::copy(&mut file, &mut stdout)
        .map_err(|error| CliError::unavailable(format!("stdout write failed: {error}")))?;
    Ok(())
}

fn file_set(root: &Path, path: &str, value: &str) -> Result<(), CliError> {
    let path = resolve_abi_path(root, path)?;
    let Some(parent) = path.parent() else {
        return Err(CliError::usage("set requires a parent directory"));
    };
    let parent_dir = open_plain_file_parent_dir(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::usage("set requires a valid file name"))?;
    let content = newline_terminated(value);

    for attempt in 0..16 {
        let temp_name = temp_file_name(attempt);
        let file_fd = match nix::fcntl::openat(
            &parent_dir,
            temp_name.as_str(),
            nix::fcntl::OFlag::O_CREAT
                | nix::fcntl::OFlag::O_EXCL
                | nix::fcntl::OFlag::O_WRONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::from_bits_truncate(0o644),
        ) {
            Ok(file_fd) => file_fd,
            Err(nix::errno::Errno::EEXIST) => continue,
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot create temp file: {error}"
                )));
            }
        };
        let mut file = fs::File::from(file_fd);
        file.write_all(content.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|error| {
                let _ignored = nix::unistd::unlinkat(
                    &parent_dir,
                    temp_name.as_str(),
                    nix::unistd::UnlinkatFlags::NoRemoveDir,
                );
                CliError::unavailable(format!("cannot write temp file: {error}"))
            })?;
        drop(file);

        nix::fcntl::renameat(&parent_dir, temp_name.as_str(), &parent_dir, file_name).map_err(
            |error| {
                let _ignored = nix::unistd::unlinkat(
                    &parent_dir,
                    temp_name.as_str(),
                    nix::unistd::UnlinkatFlags::NoRemoveDir,
                );
                CliError::unavailable(format!("cannot replace {}: {error}", path.display()))
            },
        )?;
        return parent_dir.sync_all().map_err(|error| {
            CliError::unavailable(format!("cannot sync {}: {error}", parent.display()))
        });
    }
    Err(CliError::unavailable("cannot create unique temp file"))
}

fn file_append(root: &Path, path: &str, value: &str) -> Result<(), CliError> {
    let path = resolve_abi_path(root, path)?;
    let Some(parent) = path.parent() else {
        return Err(CliError::usage("append requires a parent directory"));
    };
    let parent_dir = open_plain_file_parent_dir(parent)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::usage("append requires a valid file name"))?;
    let content = newline_terminated(value);
    let file_fd = nix::fcntl::openat(
        &parent_dir,
        file_name,
        nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_APPEND
            | nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::from_bits_truncate(0o644),
    )
    .map_err(|error| CliError::unavailable(format!("cannot append {}: {error}", path.display())))?;
    let mut file = fs::File::from(file_fd);
    if !file.metadata().is_ok_and(|metadata| metadata.is_file()) {
        return Err(CliError::unavailable(format!(
            "cannot append {}: not a regular file",
            path.display()
        )));
    }
    file.write_all(content.as_bytes()).map_err(|error| {
        CliError::unavailable(format!("cannot append {}: {error}", path.display()))
    })?;
    file.sync_all().map_err(|error| {
        CliError::unavailable(format!("cannot sync {}: {error}", path.display()))
    })?;
    parent_dir.sync_all().map_err(|error| {
        CliError::unavailable(format!("cannot sync {}: {error}", parent.display()))
    })
}

fn open_plain_file_parent_dir(path: &Path) -> Result<fs::File, CliError> {
    let mut directory = if path.is_absolute() {
        open_single_plain_file_parent_dir(Path::new("/"))?
    } else {
        open_single_plain_file_parent_dir(Path::new("."))?
    };
    for component in path.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::CurDir => {}
            std::path::Component::Normal(name) => {
                let name = name.to_str().ok_or_else(|| {
                    CliError::unavailable(format!(
                        "cannot open parent {}: invalid directory name",
                        path.display()
                    ))
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
                .map_err(|error| {
                    CliError::unavailable(format!("cannot open parent {}: {error}", path.display()))
                })?;
                directory = fs::File::from(next);
            }
            std::path::Component::ParentDir | std::path::Component::Prefix(_) => {
                return Err(CliError::unavailable(format!(
                    "cannot open parent {}: unsupported path component",
                    path.display()
                )));
            }
        }
    }
    Ok(directory)
}

fn open_single_plain_file_parent_dir(path: &Path) -> Result<fs::File, CliError> {
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(nix::libc::O_DIRECTORY | nix::libc::O_NOFOLLOW)
        .open(path)
        .map_err(|error| {
            CliError::unavailable(format!("cannot open parent {}: {error}", path.display()))
        })?;
    if !directory.metadata().is_ok_and(|metadata| metadata.is_dir()) {
        return Err(CliError::unavailable(format!(
            "cannot open parent {}: not a directory",
            path.display()
        )));
    }
    Ok(directory)
}

fn file_type(root: &Path, path: &str) -> Result<(), CliError> {
    print_line(&file_type_name(root, path)?)
}

fn file_info(root: &Path, path: &str) -> Result<(), CliError> {
    let resolved = resolve_abi_path(root, path)?;
    let metadata = fs::symlink_metadata(&resolved).map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", resolved.display()))
    })?;
    let bytes = metadata.len();
    print_line(&format!("path={}", classify_input_path(root, path)?))?;
    print_line(&format!("resolved={}", resolved.display()))?;
    print_line(&format!("type={}", file_type_name(root, path)?))?;
    print_line(&format!("fs_type={}", fs_type_name(&metadata)))?;
    print_line(&format!("bytes={bytes}"))?;
    print_line(&format!(
        "token_estimate={}",
        cortexfs_token_estimate(&resolved, bytes)
    ))?;
    print_cortexfs_xattrs(&resolved)
}

fn file_type_name(root: &Path, path: &str) -> Result<String, CliError> {
    let resolved = resolve_abi_path(root, path)?;
    let shape = classify_abi_path(&classify_input_path(root, path)?);
    if shape != "ctx.unknown" {
        return Ok(shape.to_owned());
    }

    let Some(parent) = resolved.parent() else {
        return Err(CliError::unavailable(format!(
            "unknown CortexFS path: {path}"
        )));
    };
    let parent_dir = open_plain_file_parent_dir(parent)?;
    let file_name = resolved
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| CliError::unavailable(format!("unknown CortexFS path: {path}")))?;
    let metadata = nix::sys::stat::fstatat(
        &parent_dir,
        file_name,
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| CliError::unavailable(format!("unknown CortexFS path: {path}")))?;
    let file_type = nix::sys::stat::SFlag::from_bits_truncate(metadata.st_mode);
    if file_type.contains(nix::sys::stat::SFlag::S_IFLNK) {
        Ok("ctx.symlink".to_owned())
    } else {
        Ok("ctx.ordinary".to_owned())
    }
}

fn fs_type_name(metadata: &fs::Metadata) -> &'static str {
    let file_type = metadata.file_type();
    if file_type.is_dir() {
        "directory"
    } else if file_type.is_symlink() {
        "symlink"
    } else if metadata.mode() & nix::libc::S_IFMT == nix::libc::S_IFREG {
        "regular"
    } else if file_type.is_socket() {
        "socket"
    } else if file_type.is_fifo() {
        "fifo"
    } else if file_type.is_char_device() {
        "char"
    } else if file_type.is_block_device() {
        "block"
    } else {
        "special"
    }
}

fn cortexfs_token_estimate(path: &Path, bytes: u64) -> String {
    read_xattr_string(path, "user.cortexfs.token_estimate").unwrap_or_else(|| {
        if bytes == 0 {
            "0".to_owned()
        } else {
            bytes.div_ceil(4).to_string()
        }
    })
}

fn print_cortexfs_xattrs(path: &Path) -> Result<(), CliError> {
    let Ok(names) = xattr::list(path) else {
        return Ok(());
    };
    let mut names = names
        .filter_map(|name| name.into_string().ok())
        .filter(|name| name.starts_with("user.cortexfs."))
        .collect::<Vec<_>>();
    names.sort_unstable();
    for name in names {
        if let Some(value) = read_xattr_string(path, &name) {
            print_line(&cortexfs_xattr_line(&name, &value))?;
        }
    }
    Ok(())
}

fn cortexfs_xattr_line(name: &str, value: &str) -> String {
    format!(
        "xattr.{}={}",
        terminal_safe_text(name),
        terminal_safe_text(value)
    )
}

fn read_xattr_string(path: &Path, name: &str) -> Option<String> {
    let value = xattr::get(path, name).ok()??;
    String::from_utf8(value).ok()
}
