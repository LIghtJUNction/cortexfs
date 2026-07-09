use crate::*;

macro_rules! ensure_reference_tree_ready {
    ($source:expr) => {{
        let source = $source;
        ensure_v1_reference_tree(source).map_err(|error| {
            CliError::unavailable(format!(
                "cannot bootstrap {}: {} ({error:?})",
                source.display(),
                error.errno(),
            ))
        })?;
        ensure_v1_runtime_models(source).map_err(|error| {
            CliError::unavailable(format!(
                "cannot materialize runtime models {}: {} ({error:?})",
                source.display(),
                error.errno(),
            ))
        })?;
    }};
}

pub(crate) fn bootstrap_reference_tree(source: Option<&Path>) -> Result<(), CliError> {
    let source = match source {
        Some(path) => path.to_path_buf(),
        None => default_source_root()?,
    };
    ensure_reference_tree_ready!(&source);
    for line in [
        format!("source={}", source.display()),
        "agents=architect,coder,reviewer".to_owned(),
        "agent.coder.parent=agent:architect".to_owned(),
        "agent.coder.model=main".to_owned(),
        "agent.coder.workspace=/workspace".to_owned(),
        "agent.coder.tools=tsh,fs.read,fs.write,fs.replace,shell.exec".to_owned(),
        "agent.coder.chat=ctx agent start coder && ctx agent chat coder".to_owned(),
    ] {
        print_line(&line)?;
    }
    Ok(())
}

pub(crate) fn mount_reference_tree(
    root: &Path,
    source: Option<&Path>,
    mountpoint: Option<&Path>,
) -> Result<(), CliError> {
    let source = match source {
        Some(path) => path.to_path_buf(),
        None => default_source_root()?,
    };
    let mountpoint = mountpoint.unwrap_or(root);

    ensure_reference_tree_ready!(&source);
    create_plain_mountpoint_dir(mountpoint)?;
    ensure_plain_mountpoint_dir(mountpoint)?;
    let mountpoint = absolute_existing_path(mountpoint).map_err(|error| {
        CliError::unavailable(format!(
            "cannot resolve mountpoint {}: {error}",
            mountpoint.display()
        ))
    })?;
    if is_mount_point(&mountpoint).unwrap_or(false) {
        return Err(CliError::unavailable(format!(
            "already mounted: {}",
            mountpoint.display()
        )));
    }

    let mount_bin = cortexfs_mount_bin();
    spawn_mount_process(&mount_bin, &source, &mountpoint)?;

    for _attempt in 0..20 {
        if is_mount_point(&mountpoint).unwrap_or(false) {
            print_line(&format!("mounted={}", mountpoint.display()))?;
            print_line(&format!("source={}", source.display()))?;
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    Err(CliError::unavailable(format!(
        "mount did not become ready: {}",
        mountpoint.display()
    )))
}

pub(crate) fn create_plain_mountpoint_dir(mountpoint: &Path) -> Result<(), CliError> {
    create_plain_directory(
        mountpoint,
        0o755,
        "mountpoint is not plain directory",
        "mountpoint path contains non-directory entry",
        "invalid mountpoint directory name",
    )
    .map_err(|error| {
        CliError::unavailable(format!(
            "cannot create mountpoint {}: {error}",
            mountpoint.display()
        ))
    })
}

pub(crate) fn ensure_plain_mountpoint_dir(mountpoint: &Path) -> Result<(), CliError> {
    let directory = open_plain_file_parent_dir(mountpoint)?;
    let metadata = directory.metadata().map_err(|error| {
        CliError::unavailable(format!(
            "cannot stat mountpoint {}: {error}",
            mountpoint.display()
        ))
    })?;
    if metadata.is_dir() {
        Ok(())
    } else {
        Err(CliError::unavailable(format!(
            "mountpoint is not a plain directory: {}",
            mountpoint.display()
        )))
    }
}

pub(crate) fn default_source_root() -> Result<PathBuf, CliError> {
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

pub(crate) fn cortexfs_mount_bin() -> PathBuf {
    if let Ok(current) = env::current_exe()
        && let Some(sibling) = plain_sibling_mount_bin(&current)
    {
        return sibling;
    }
    PathBuf::from(CORTEXFS_MOUNT_PROGRAM)
}

pub(crate) fn plain_sibling_mount_bin(current_exe: &Path) -> Option<PathBuf> {
    let sibling = current_exe.parent()?.join("cortexfs-mount");
    let metadata = sibling.symlink_metadata().ok()?;
    (metadata.is_file()
        && !metadata.file_type().is_symlink()
        && metadata.permissions().mode() & 0o111 != 0)
        .then_some(sibling)
}

const CORTEXFS_MOUNT_PROGRAM: &str = "/usr/bin/cortexfs-mount";
const TRUSTED_SETSID_BIN: &str = "/usr/bin/setsid";

pub(crate) fn spawn_mount_process(
    mount_bin: &Path,
    source: &Path,
    mountpoint: &Path,
) -> Result<(), CliError> {
    match spawn_null(detached_mount_command(mount_bin, source, mountpoint)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            spawn_null(direct_mount_command(mount_bin, source, mountpoint)).map_err(|error| {
                CliError::unavailable(format!("cannot start {}: {error}", mount_bin.display()))
            })
        }
        Err(error) => Err(CliError::unavailable(format!(
            "cannot start {}: {error}",
            mount_bin.display()
        ))),
    }
}

pub(crate) fn detached_mount_command(
    mount_bin: &Path,
    source: &Path,
    mountpoint: &Path,
) -> ProcessCommand {
    let mut command = ProcessCommand::new(TRUSTED_SETSID_BIN);
    command
        .arg("-f")
        .arg(mount_bin)
        .arg("--source")
        .arg(source)
        .arg(mountpoint)
        .env_clear()
        .env("PATH", "/usr/bin:/bin");
    command
}

pub(crate) fn direct_mount_command(
    mount_bin: &Path,
    source: &Path,
    mountpoint: &Path,
) -> ProcessCommand {
    let mut command = ProcessCommand::new(mount_bin);
    command
        .arg("--source")
        .arg(source)
        .arg(mountpoint)
        .env_clear()
        .env("PATH", "/usr/bin:/bin");
    command
}

pub(crate) fn spawn_null(mut command: ProcessCommand) -> io::Result<()> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_child| ())
}
