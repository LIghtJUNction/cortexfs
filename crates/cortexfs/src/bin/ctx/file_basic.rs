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
        FileCommand::Cat => file_cat(root, &args.path),
        FileCommand::Set => {
            let Some(value) = args.value.as_deref() else {
                return Err(CliError::usage("file set requires a value"));
            };
            file_set(root, &args.path, value)
        }
        FileCommand::Append => {
            let Some(value) = args.value.as_deref() else {
                return Err(CliError::usage("file append requires a value"));
            };
            file_append(root, &args.path, value)
        }
        FileCommand::Check => file_check(root, &args.path),
        FileCommand::Classify => file_classify(root, &args.path),
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
        return Err(CliError::usage("file set requires a parent directory"));
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

fn file_classify(root: &Path, path: &str) -> Result<(), CliError> {
    let resolved = resolve_abi_path(root, path)?;
    let shape = classify_abi_path(&classify_input_path(root, path)?);
    if shape != "ctx.unknown" {
        return print_line(shape);
    }

    if fs::symlink_metadata(&resolved).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return print_line("ctx.symlink");
    }

    if resolved.exists() {
        return print_line("ctx.ordinary");
    }

    Err(CliError::unavailable(format!(
        "unknown CortexFS path: {path}"
    )))
}
