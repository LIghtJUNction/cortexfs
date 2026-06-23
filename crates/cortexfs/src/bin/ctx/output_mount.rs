fn print_help() -> Result<(), CliError> {
    print_lines(&[
        "ctx - CortexFS filesystem management CLI",
        "",
        "usage:",
        "  ctx [--root PATH] status",
        "  ctx [--root PATH] abi",
        "  ctx [--root PATH] env",
        "  ctx [--root PATH] root",
        "  ctx bootstrap [SOURCE]",
        "  ctx [--root PATH] mount [--source SOURCE] [MOUNTPOINT]",
        "  ctx [--root PATH] ls [PATH|model|agent|tool]",
        "  ctx [--root PATH] which model|agent|tool NAME",
        "  ctx [--root PATH] path shared NAME",
        "  ctx [--root PATH] history AGENT [SESSION]",
        "  ctx [--root PATH] latest AGENT [SESSION]",
        "  ctx [--root PATH] resume AGENT [SESSION]",
        "  ctx [--root PATH] send AGENT SESSION INPUT",
        "  ctx [--root PATH] ping model/NAME|agent/NAME",
        "  ctx [--root PATH] cancel model/NAME|agent/NAME RUN",
        "  ctx [--root PATH] exec model/NAME|agent/NAME|tool/NAME [ARG...]",
        "  ctx [--root PATH] file PATH",
        "  ctx [--root PATH] file cat PATH",
        "  ctx [--root PATH] file set PATH VALUE",
        "  ctx [--root PATH] file append PATH VALUE",
        "  ctx [--root PATH] file check PATH",
        "  ctx [--root PATH] file classify PATH",
        "  ctx [--root PATH] doctor",
        "  ctx validate-name NAME",
        "",
        "principles:",
        "  ctx is a thin Unix client over /ctx",
        "  ctx does not manage providers, API formats, or private sessions",
    ])
}

fn print_abi() -> Result<(), CliError> {
    print_line("root=/ctx")?;
    print_line("entries=status bin model agent tool home shared")?;
    print_line("exec=model agent tool")?;
    print_line("socket=name.sock")?;
    print_line("control=name.d")?;
    print_line("policy=allow <subject_type> <object_class>:<object_name> <permission>")
}

fn print_env(root: &Path) -> Result<(), CliError> {
    let home = env::var("CTX_HOME").unwrap_or_else(|_| format!("{}/home/$(id -u)", root.display()));
    let path =
        env::var("CTX_PATH").unwrap_or_else(|_| format!("{}/tool:{home}/tool", root.display()));
    print_line(&format!(
        "export CTX_ROOT={}",
        shell_quote(&root.display().to_string())
    ))?;
    print_line(&format!("export CTX_HOME={}", shell_quote(&home)))?;
    print_line(&format!("export CTX_PATH={}", shell_quote(&path)))?;
    print_line(&format!("export PATH={}/bin:$PATH", root.display()))
}

fn print_status(root: &Path) -> Result<(), CliError> {
    let exists = root.exists();
    let is_dir = root.is_dir();
    let mounted = is_mount_point(root).unwrap_or(false);

    print_line(&format!("root={}", root.display()))?;
    print_line(&format!("exists={}", bool_text(exists)))?;
    print_line(&format!("dir={}", bool_text(is_dir)))?;
    print_line(&format!("mounted={}", bool_text(mounted)))?;

    for entry in ROOT_ENTRIES {
        let present = root.join(entry).exists();
        print_line(&format!("{entry}={}", bool_text(present)))?;
    }

    Ok(())
}

fn bootstrap_reference_tree(source: Option<&Path>) -> Result<(), CliError> {
    let source = match source {
        Some(path) => path.to_path_buf(),
        None => default_source_root()?,
    };
    ensure_v1_reference_tree(&source).map_err(|error| {
        CliError::unavailable(format!(
            "cannot bootstrap {}: {}",
            source.display(),
            error.errno()
        ))
    })?;
    print_line(&format!("source={}", source.display()))
}

fn mount_reference_tree(
    root: &Path,
    source: Option<&Path>,
    mountpoint: Option<&Path>,
) -> Result<(), CliError> {
    let source = match source {
        Some(path) => path.to_path_buf(),
        None => default_source_root()?,
    };
    let mountpoint = mountpoint.unwrap_or(root);

    ensure_v1_reference_tree(&source).map_err(|error| {
        CliError::unavailable(format!(
            "cannot bootstrap {}: {}",
            source.display(),
            error.errno()
        ))
    })?;
    fs::create_dir_all(mountpoint).map_err(|error| {
        CliError::unavailable(format!(
            "cannot create mountpoint {}: {error}",
            mountpoint.display()
        ))
    })?;
    if is_mount_point(mountpoint).unwrap_or(false) {
        return Err(CliError::unavailable(format!(
            "already mounted: {}",
            mountpoint.display()
        )));
    }

    let mount_bin = cortexfs_mount_bin();
    spawn_mount_process(&mount_bin, &source, mountpoint)?;

    for _attempt in 0..20 {
        if is_mount_point(mountpoint).unwrap_or(false) {
            print_line(&format!("mounted={}", mountpoint.display()))?;
            print_line(&format!("source={}", source.display()))?;
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    Err(CliError::unavailable(format!(
        "mount did not become ready: {}",
        mountpoint.display()
    )))
}

fn default_source_root() -> Result<PathBuf, CliError> {
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(data_home).join("cortexfs").join("v1-root"));
    }
    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("cortexfs")
            .join("v1-root"));
    }
    Err(CliError::unavailable(
        "cannot choose default source root without HOME or XDG_DATA_HOME",
    ))
}

fn cortexfs_mount_bin() -> PathBuf {
    if let Ok(current) = env::current_exe()
        && let Some(dir) = current.parent()
    {
        let sibling = dir.join("cortexfs-mount");
        if sibling.is_file() {
            return sibling;
        }
    }
    PathBuf::from("cortexfs-mount")
}

fn spawn_mount_process(mount_bin: &Path, source: &Path, mountpoint: &Path) -> Result<(), CliError> {
    let mut detached = ProcessCommand::new("setsid");
    detached
        .arg("-f")
        .arg(mount_bin)
        .arg("--source")
        .arg(source)
        .arg(mountpoint);
    match spawn_null(detached) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let mut direct = ProcessCommand::new(mount_bin);
            direct.arg("--source").arg(source).arg(mountpoint);
            spawn_null(direct).map_err(|error| {
                CliError::unavailable(format!("cannot start {}: {error}", mount_bin.display()))
            })
        }
        Err(error) => Err(CliError::unavailable(format!(
            "cannot start {}: {error}",
            mount_bin.display()
        ))),
    }
}

fn spawn_null(mut command: ProcessCommand) -> io::Result<()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_child| ())
}
