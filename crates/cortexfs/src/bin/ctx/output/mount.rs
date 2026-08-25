use crate::*;

macro_rules! ensure_reference_tree_ready {
    ($source:expr) => {{
        let source = $source;
        ensure_reference_tree(source).map_err(|error| {
            CliError::unavailable(format!(
                "cannot bootstrap {}: {} ({error:?})",
                source.display(),
                error.errno(),
            ))
        })?;
        ensure_runtime_models(source).map_err(|error| {
            CliError::unavailable(format!(
                "cannot materialize runtime models {}: {} ({error:?})",
                source.display(),
                error.errno(),
            ))
        })?;
    }};
}

pub(crate) fn bootstrap_reference_tree(
    source: Option<&Path>,
    dry_run: bool,
    check: bool,
) -> Result<(), CliError> {
    if let Some(source) = source {
        return bootstrap_reference_tree_at(source, dry_run, check);
    }
    bootstrap_reference_tree_default(&default_source_parent()?, dry_run, check)
}

pub(crate) fn bootstrap_reference_tree_default(
    parent: &Path,
    dry_run: bool,
    check: bool,
) -> Result<(), CliError> {
    let source = if check || dry_run {
        parent.join("root")
    } else {
        adopt_default_source_root(parent)?
    };
    bootstrap_reference_tree_at(&source, dry_run, check)
}

fn bootstrap_reference_tree_at(source: &Path, dry_run: bool, check: bool) -> Result<(), CliError> {
    // An explicit SOURCE may be the documented `storage/current` generation
    // symlink; bootstrap operates on the resolved plain directory behind it.
    let resolved = source
        .symlink_metadata()
        .ok()
        .filter(|metadata| metadata.file_type().is_symlink())
        .and_then(|_metadata| source.canonicalize().ok());
    let source = resolved.as_deref().unwrap_or(source);
    if check {
        return print_bootstrap_check(source);
    }
    if dry_run {
        return print_bootstrap_dry_run(source);
    }
    ensure_reference_tree_ready!(source);
    let state = read_bootstrap_state(source);
    for line in [
        format!("source={}", source.display()),
        format!(
            "tree_version={}",
            state
                .as_ref()
                .map_or(REFERENCE_TREE_VERSION, |value| value.tree_version)
        ),
        format!(
            "migrations={}",
            state.as_ref().map_or_else(
                || "none".to_owned(),
                |value| {
                    if value.applied_migrations.is_empty() {
                        "none".to_owned()
                    } else {
                        value.applied_migrations.join(",")
                    }
                }
            )
        ),
        BOOTSTRAP_REFERENCE_AGENT_SUMMARY_LINE.to_owned(),
    ] {
        print_line(&line)?;
    }
    Ok(())
}

pub(crate) const BOOTSTRAP_REFERENCE_AGENT_SUMMARY_LINE: &str =
    "agents=architect,executor,product-manager";

pub(crate) fn print_bootstrap_check(source: &Path) -> Result<(), CliError> {
    print_line(&format!("source={}", source.display()))?;
    let plan = plan_reference_tree_upgrade(source);
    for line in format_bootstrap_plan_lines(&plan) {
        // check mode uses neutral verbs
        let line = line
            .replace("would_apply ", "pending ")
            .replace("would_skip ", "keep ")
            .replace("would_ensure ", "missing ")
            .replace("would_write ", "state ");
        print_line(&line)?;
    }
    let retired = list_present_retired_reference_agents(source);
    if retired.is_empty() {
        print_line("retired=none")?;
    } else {
        print_line(&format!("retired={}", retired.join(",")))?;
    }
    Ok(())
}

pub(crate) fn print_bootstrap_dry_run(source: &Path) -> Result<(), CliError> {
    print_line(&format!("source={}", source.display()))?;
    print_line("mode=dry-run")?;
    let plan = plan_reference_tree_upgrade(source);
    let rejects_version = plan
        .actions
        .iter()
        .any(|action| matches!(action, BootstrapAction::RejectVersion { .. }));
    for line in format_bootstrap_plan_lines(&plan) {
        print_line(&line)?;
    }
    if rejects_version {
        print_line("note=no files written; source tree is newer than this binary")?;
    } else {
        print_line("note=no files written; run ctx bootstrap without --dry-run to apply")?;
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
        None => adopt_default_source_root(&default_source_parent()?)?,
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

fn default_source_parent() -> Result<PathBuf, CliError> {
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(data_home).join("cortexfs"));
    }
    if let Some(home) = env::var_os("HOME") {
        return Ok(PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("cortexfs"));
    }
    Err(CliError::unavailable(
        "cannot choose default source root without HOME or XDG_DATA_HOME",
    ))
}

pub(crate) fn adopt_default_source_root(parent: &Path) -> Result<PathBuf, CliError> {
    const LEGACY_ROOT: &str = "v1-root";
    const ROOT: &str = "root";

    let canonical = parent.join(ROOT);
    let directory = match open_plain_directory(parent) {
        Ok(directory) => directory,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(canonical),
        Err(error) => {
            return Err(CliError::unavailable(format!(
                "cannot inspect default source directory {}: {error}",
                parent.display()
            )));
        }
    };
    let stat = |name| match nix::sys::stat::fstatat(
        &directory,
        name,
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    ) {
        Ok(stat) => Ok(Some(stat)),
        Err(nix::errno::Errno::ENOENT) => Ok(None),
        Err(error) => Err(CliError::unavailable(format!(
            "cannot inspect default source {}: {error}",
            parent.join(name).display()
        ))),
    };
    let legacy = stat(LEGACY_ROOT)?;
    let current = stat(ROOT)?;
    if legacy.is_some() && current.is_some() {
        return Err(CliError::unavailable(format!(
            "default source conflict: both {} and {} exist",
            parent.join(LEGACY_ROOT).display(),
            canonical.display()
        )));
    }
    let Some(legacy) = legacy else {
        return Ok(canonical);
    };
    if !nix::sys::stat::SFlag::from_bits_truncate(legacy.st_mode)
        .contains(nix::sys::stat::SFlag::S_IFDIR)
    {
        return Err(CliError::unavailable(format!(
            "legacy default source is not a plain directory: {}",
            parent.join(LEGACY_ROOT).display()
        )));
    }
    nix::fcntl::renameat2(
        &directory,
        LEGACY_ROOT,
        &directory,
        ROOT,
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|error| {
        if error == nix::errno::Errno::EEXIST {
            CliError::unavailable(format!(
                "default source conflict: both {} and {} exist",
                parent.join(LEGACY_ROOT).display(),
                canonical.display()
            ))
        } else {
            CliError::unavailable(format!(
                "cannot adopt legacy default source {} as {}: {error}",
                parent.join(LEGACY_ROOT).display(),
                canonical.display()
            ))
        }
    })?;
    directory.sync_all().map_err(|error| {
        CliError::unavailable(format!(
            "cannot sync adopted default source {}: {error}",
            parent.display()
        ))
    })?;
    Ok(canonical)
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

const CORTEXFS_MOUNT_PROGRAM: &str = cortexfs::support::command::CORTEXFS_MOUNT;
const TRUSTED_SETSID_BIN: &str = cortexfs::support::command::SETSID;

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
        .env("PATH", cortexfs::support::command::TRUSTED_PATH);
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
        .env("PATH", cortexfs::support::command::TRUSTED_PATH);
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
