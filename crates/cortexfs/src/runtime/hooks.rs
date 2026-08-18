#![allow(
    clippy::redundant_pub_crate,
    unreachable_pub,
    reason = "hook execution is an internal runtime stage"
)]

use std::fs;
use std::path::{Path, PathBuf};

use crate::AgentUnixIdentity;
use crate::abi::path::is_object_name;
use crate::runtime::hookabi::{HookError, HookInvocation};
use crate::runtime::hookexec::run_one_hook;
use crate::support::plain::open_plain_directory;

pub(crate) fn run_agent_hooks(
    control_dir: &Path,
    invocation: &HookInvocation<'_>,
    identity: &AgentUnixIdentity,
) -> Result<(), HookError> {
    let directory = control_dir.join("hooks").join(invocation.phase.directory());
    if let Err(error) = open_plain_directory(&directory) {
        return if error.kind() == std::io::ErrorKind::NotFound {
            Ok(())
        } else {
            Err(HookError::new("EIO", invocation.phase.directory()))
        };
    }
    let mut hooks = fs::read_dir(&directory)
        .map_err(|_error| HookError::new("EIO", invocation.phase.directory()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<PathBuf>, _>>()
        .map_err(|_error| HookError::new("EIO", invocation.phase.directory()))?;
    hooks.sort();
    if hooks.len() > crate::runtime::hookabi::MAX_HOOKS {
        return Err(HookError::new("E2BIG", invocation.phase.directory()));
    }
    let frame = serde_json::json!({
        "abi": crate::runtime::hookabi::HOOK_ABI,
        "phase": invocation.phase.as_str(),
        "action": invocation.action,
        "agent": invocation.agent,
        "run": invocation.run,
        "step": invocation.step,
        "tool": invocation.tool,
        "status": invocation.status,
    })
    .to_string();
    for path in hooks {
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("hook");
        if !is_object_name(name) {
            return Err(HookError::new("EINVAL", name));
        }
        run_one_hook(&path, frame.as_bytes(), identity)?;
    }
    Ok(())
}
