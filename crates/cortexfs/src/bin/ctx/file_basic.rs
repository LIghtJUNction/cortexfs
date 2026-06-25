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
    if !is_executable_file(&path) {
        return Err(CliError::unavailable(format!(
            "object is not executable: {}",
            path.display()
        )));
    }

    let status = ProcessCommand::new(&path)
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
    let mut file = fs::File::open(path).map_err(|error| {
        CliError::unavailable(format!("cannot read {}: {error}", path.display()))
    })?;
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
    let temp = parent.join(temp_file_name());
    let content = newline_terminated(value);

    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp)
        .map_err(|error| {
            CliError::unavailable(format!("cannot create {}: {error}", temp.display()))
        })?;
    file.write_all(content.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|error| {
            CliError::unavailable(format!("cannot write {}: {error}", temp.display()))
        })?;

    fs::rename(&temp, &path).map_err(|error| {
        let _ignored = fs::remove_file(&temp);
        CliError::unavailable(format!("cannot replace {}: {error}", path.display()))
    })
}

fn file_append(root: &Path, path: &str, value: &str) -> Result<(), CliError> {
    let path = resolve_abi_path(root, path)?;
    let content = newline_terminated(value);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| {
            CliError::unavailable(format!("cannot append {}: {error}", path.display()))
        })?;
    file.write_all(content.as_bytes()).map_err(|error| {
        CliError::unavailable(format!("cannot append {}: {error}", path.display()))
    })
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

    if fs::symlink_metadata(&resolved).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Ok("ctx.symlink".to_owned());
    }

    if resolved.exists() {
        return Ok("ctx.ordinary".to_owned());
    }

    Err(CliError::unavailable(format!(
        "unknown CortexFS path: {path}"
    )))
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
            print_line(&format!("xattr.{name}={value}"))?;
        }
    }
    Ok(())
}

fn read_xattr_string(path: &Path, name: &str) -> Option<String> {
    let value = xattr::get(path, name).ok()??;
    String::from_utf8(value).ok()
}
