use std::path::{Path, PathBuf};

use crate::agent::loopconfig::AgentLoop;
use crate::agent::runtime::AgentRuntimeViewError;
use crate::agent::view::resolve_agent_runtime_control_dir;
use crate::cli::nofollow::open_executable_no_follow;
use crate::support::plain::read_small_text_file;

/// Resolves the executable used for hosted envelope steps.
pub fn resolve_agent_loop_executable_for_agent(
    ctx_root: &Path,
    agent_name: &str,
) -> Result<PathBuf, AgentRuntimeViewError> {
    let control_dir = resolve_agent_runtime_control_dir(ctx_root, agent_name)?;
    let default = cortexfs_paths::agent_path(ctx_root, agent_name);
    Ok(resolve_agent_loop_executable(&control_dir, &default))
}

/// Returns the loop driver executable for one agent run.
///
/// When `loop` names a custom object and `loop.d/<name>` is an executable
/// regular file, that file replaces the default `agent/<name>` executable for
/// hosted envelope steps.
#[must_use]
pub fn resolve_agent_loop_executable(control_dir: &Path, default: &Path) -> PathBuf {
    let loop_kind = match read_small_text_file(&control_dir.join("loop"), 256) {
        Ok(content) => AgentLoop::parse(&content).unwrap_or_default(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => AgentLoop::default(),
        Err(_error) => return default.to_path_buf(),
    };
    let AgentLoop::Custom(name) = loop_kind else {
        return default.to_path_buf();
    };
    let custom = control_dir.join("loop.d").join(&name);
    if open_executable_no_follow(&custom).is_ok() {
        custom
    } else {
        default.to_path_buf()
    }
}
