use crate::*;
use cortexfs::agent::stop::{TempCleanupEntry, TempCleanupPlan};

fn plan_temp_cleanup(root: &Path, name: &str) -> Result<TempCleanupPlan, CliError> {
    let agent_root = root.join("agent");
    preflight_cleanup_directory(&agent_root)?;

    let mut entries = Vec::new();
    for path in [
        agent_object_path(root, name),
        agent_socket_path(root, name)?,
    ] {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                return Err(CliError::unavailable(format!(
                    "temp agent path is not a file or socket: {}",
                    path.display()
                )));
            }
            Ok(metadata) => entries.push(TempCleanupEntry {
                path,
                directory: false,
                dev: metadata.dev(),
                ino: metadata.ino(),
                kind: metadata.mode() & libc::S_IFMT,
            }),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot stat {}: {error}",
                    path.display()
                )));
            }
        }
    }
    plan_temp_cleanup_tree(&agent_control_dir(root, name), &mut entries)?;
    Ok(TempCleanupPlan { entries })
}

fn plan_temp_cleanup_tree(
    directory: &Path,
    entries: &mut Vec<TempCleanupEntry>,
) -> Result<(), CliError> {
    preflight_cleanup_directory(directory)?;
    for name in read_dir_names(directory)? {
        let path = directory.join(name);
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
        })?;
        if metadata.file_type().is_dir() {
            plan_temp_cleanup_tree(&path, entries)?;
        } else {
            entries.push(TempCleanupEntry {
                path,
                directory: false,
                dev: metadata.dev(),
                ino: metadata.ino(),
                kind: metadata.mode() & libc::S_IFMT,
            });
        }
    }
    let metadata = fs::symlink_metadata(directory).map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", directory.display()))
    })?;
    entries.push(TempCleanupEntry {
        path: directory.to_path_buf(),
        directory: true,
        dev: metadata.dev(),
        ino: metadata.ino(),
        kind: metadata.mode() & libc::S_IFMT,
    });
    Ok(())
}

fn preflight_cleanup_directory(path: &Path) -> Result<(), CliError> {
    let directory = open_plain_directory(path).map_err(|error| {
        CliError::unavailable(format!("cannot open {}: {error}", path.display()))
    })?;
    let metadata = directory.metadata().map_err(|error| {
        CliError::unavailable(format!("cannot stat {}: {error}", path.display()))
    })?;
    let fuse = cortexfs::support::plain::is_fuse(&directory).map_err(|error| {
        CliError::unavailable(format!(
            "cannot inspect filesystem for {}: {error}",
            path.display()
        ))
    })?;
    let uid = nix::unistd::Uid::effective().as_raw();
    if uid != 0
        && !fuse
        && (metadata.uid() != uid || metadata.permissions().mode() & 0o300 != 0o300)
    {
        return Err(CliError::unavailable(format!(
            "temp cleanup directory is not owner-writable: {}",
            path.display()
        )));
    }
    Ok(())
}

fn execute_temp_cleanup(plan: TempCleanupPlan) -> Result<(), CliError> {
    for entry in plan.entries {
        let result = if entry.directory {
            fs::remove_dir(&entry.path)
        } else {
            fs::remove_file(&entry.path)
        };
        result.map_err(|error| {
            CliError::unavailable(format!("cannot remove {}: {error}", entry.path.display()))
        })?;
    }
    Ok(())
}

pub(crate) fn remove_temp_agent_object(root: &Path, child: &str) -> Result<(), CliError> {
    execute_temp_cleanup(plan_temp_cleanup(root, child)?)
}

pub(crate) fn write_agent_control_plain(path: &Path, content: &str) -> Result<(), CliError> {
    atomic_replace_text_preserving_metadata(path, content)
        .map_err(|error| CliError::unavailable(format!("cannot write {}: {error}", path.display())))
}

pub(crate) fn write_agent_session_plain(path: &Path, content: &str) -> Result<(), CliError> {
    for _attempt in 0..2 {
        match atomic_replace_text_preserving_metadata(path, content) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match atomic_create_text_with_mode(path, content, 0o600) {
                    Ok(()) => return Ok(()),
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(error) => {
                        return Err(CliError::unavailable(format!(
                            "cannot write {}: {error}",
                            path.display()
                        )));
                    }
                }
            }
            Err(error) => {
                return Err(CliError::unavailable(format!(
                    "cannot write {}: {error}",
                    path.display()
                )));
            }
        }
    }
    Err(CliError::unavailable(format!(
        "cannot write {}: target changed during creation",
        path.display()
    )))
}

pub(crate) fn append_agent_log_event(path: &Path, event: &str) -> Result<(), CliError> {
    if let Ok(metadata) = fs::symlink_metadata(path)
        && metadata.file_type().is_symlink()
    {
        return Err(CliError::unavailable(format!(
            "refusing symlink log file: {}",
            path.display()
        )));
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| {
            CliError::unavailable(format!("cannot open {}: {error}", path.display()))
        })?;
    writeln!(file, "{event}")
        .map_err(|error| CliError::unavailable(format!("cannot write {}: {error}", path.display())))
}

pub(crate) fn agent_lifecycle_tool(
    root: &Path,
    name: &str,
    request: &str,
) -> Result<ExitCode, CliError> {
    let Some(hit) = ctx_tool_path(root)?.find(name).map_err(tool_path_error)? else {
        return Err(CliError::unavailable(format!(
            "agent lifecycle tool is not available: tool/{name}"
        )));
    };
    let executable = open_executable_no_follow(hit.path())?;
    let status = agent_lifecycle_tool_command(root, &proc_fd_path(&executable))
        .arg(request)
        .status()
        .map_err(|error| {
            CliError::unavailable(format!("cannot exec {}: {error}", hit.path().display()))
        })?;
    Ok(status
        .code()
        .and_then(|code| u8::try_from(code).ok())
        .map_or_else(|| ExitCode::from(70), ExitCode::from))
}

pub(crate) fn agent_lifecycle_tool_exists(root: &Path, name: &str) -> Result<bool, CliError> {
    Ok(ctx_tool_path(root)?
        .find(name)
        .map_err(tool_path_error)?
        .is_some())
}

pub(crate) fn agent_lifecycle_tool_command(root: &Path, path: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new(path);
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("CTX_ROOT", root);
    command
}
