use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use crate::AgentUnixIdentity;
use crate::cli::nofollow::open_executable_no_follow;
use crate::cli::procfd::proc_fd_path;
use crate::runtime::hookabi::{HOOK_ABI, HookError};
use crate::runtime::socket::command_for_agent_identity;
use crate::support::command::TRUSTED_PATH;
use crate::support::process::{CappedOutputError, CappedOutputWait, wait_capped_child_output};

const MAX_OUTPUT_BYTES: usize = 16 * 1024;
const TIMEOUT: Duration = Duration::from_secs(2);

pub(super) fn run_one_hook(
    path: &Path,
    frame: &[u8],
    identity: &AgentUnixIdentity,
) -> Result<(), HookError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("hook");
    let executable =
        open_executable_no_follow(path).map_err(|_error| HookError::new("EACCES", name))?;
    if executable
        .metadata()
        .map_err(|_error| HookError::new("EIO", name))?
        .permissions()
        .mode()
        & 0o111
        == 0
    {
        return Err(HookError::new("EACCES", name));
    }
    let mut command = command_for_agent_identity(proc_fd_path(&executable), identity);
    command
        .env_clear()
        .env("PATH", TRUSTED_PATH)
        .env("CTX_HOOK_ABI", HOOK_ABI)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command
        .spawn()
        .map_err(|_error| HookError::new("EIO", name))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| HookError::new("EIO", name))?;
    if stdin
        .write_all(frame)
        .and_then(|()| stdin.write_all(b"\n"))
        .is_err()
    {
        crate::support::process::terminate_process_group(&mut child);
        return Err(HookError::new("EIO", name));
    }
    drop(stdin);
    let result = wait_capped_child_output(
        &mut child,
        CappedOutputWait {
            max_output_bytes: MAX_OUTPUT_BYTES,
            timeout: TIMEOUT,
            capture_stderr: true,
            drain_timeout: Some(Duration::from_millis(200)),
            terminate_group_after_exit: true,
        },
        || false,
    );
    match result {
        Ok(output) if output.status.success() => Ok(()),
        Ok(_) => Err(HookError::new("EACCES", name)),
        Err(CappedOutputError::TimedOut) => Err(HookError::new("ETIMEDOUT", name)),
        Err(CappedOutputError::ExceededLimit) => Err(HookError::new("EOVERFLOW", name)),
        Err(_) => Err(HookError::new("EIO", name)),
    }
}
