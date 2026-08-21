use crate::*;
use cortexfs::agent::stop::{execute_agent_cleanup, plan_agent_cleanup};

pub(crate) fn remove_temp_agent_object(root: &Path, child: &str) -> Result<(), CliError> {
    let plan = plan_agent_cleanup(root, child, nix::unistd::Uid::effective().as_raw())
        .map_err(|error| CliError::unavailable(error.to_string()))?;
    execute_agent_cleanup(plan).map_err(|error| CliError::unavailable(error.to_string()))
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
        .env("PATH", cortexfs::support::command::TRUSTED_PATH)
        .env("CTX_ROOT", root);
    command
}
