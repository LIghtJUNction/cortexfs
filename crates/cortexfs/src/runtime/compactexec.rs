use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use crate::AgentUnixIdentity;
use crate::cli::nofollow::open_executable_no_follow;
use crate::cli::procfd::proc_fd_path;
use crate::runtime::compactabi::{
    COMPACT_ABI, CompactError, CompactInvocation, MAX_COMPACT_INPUT_BYTES,
    MAX_COMPACT_OUTPUT_BYTES, compact_frame,
};
use crate::runtime::socket::command_for_agent_identity;
use crate::support::command::TRUSTED_PATH;
use crate::support::process::{CappedOutputError, CappedOutputWait, wait_capped_child_output};
use cortexfs_context::Message;

const TIMEOUT: Duration = Duration::from_secs(5);

#[allow(
    clippy::redundant_pub_crate,
    reason = "compactexec stays crate-local while agent prompt code calls it through runtime"
)]
pub(crate) fn run_custom_compact(
    path: &Path,
    invocation: &CompactInvocation<'_>,
    omitted: &[Message],
    identity: &AgentUnixIdentity,
) -> Result<String, CompactError> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("compact");
    let frame = compact_frame(invocation, omitted);
    if frame.len() > MAX_COMPACT_INPUT_BYTES {
        return Err(CompactError::new("E2BIG", name));
    }
    let executable =
        open_executable_no_follow(path).map_err(|_error| CompactError::new("EACCES", name))?;
    if executable
        .metadata()
        .map_err(|_error| CompactError::new("EIO", name))?
        .permissions()
        .mode()
        & 0o111
        == 0
    {
        return Err(CompactError::new("EACCES", name));
    }
    let mut command = command_for_agent_identity(proc_fd_path(&executable), identity);
    command
        .env_clear()
        .env("PATH", TRUSTED_PATH)
        .env("CTX_COMPACT_ABI", COMPACT_ABI)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command
        .spawn()
        .map_err(|_error| CompactError::new("EIO", name))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| CompactError::new("EIO", name))?;
    if stdin
        .write_all(frame.as_bytes())
        .and_then(|()| stdin.write_all(b"\n"))
        .is_err()
    {
        crate::support::process::terminate_process_group(&mut child);
        return Err(CompactError::new("EIO", name));
    }
    drop(stdin);
    let result = wait_capped_child_output(
        &mut child,
        CappedOutputWait {
            max_output_bytes: MAX_COMPACT_OUTPUT_BYTES,
            timeout: TIMEOUT,
            capture_stderr: true,
            drain_timeout: Some(Duration::from_millis(200)),
            terminate_group_after_exit: true,
        },
        || false,
    );
    match result {
        Ok(output) if output.status.success() => {
            let summary = String::from_utf8(output.stdout)
                .map_err(|_error| CompactError::new("EINVAL", name))?;
            if summary.contains('\0') {
                return Err(CompactError::new("EINVAL", name));
            }
            Ok(summary.trim().to_owned())
        }
        Ok(_) => Err(CompactError::new("EACCES", name)),
        Err(CappedOutputError::TimedOut) => Err(CompactError::new("ETIMEDOUT", name)),
        Err(CappedOutputError::ExceededLimit) => Err(CompactError::new("EOVERFLOW", name)),
        Err(_) => Err(CompactError::new("EIO", name)),
    }
}
