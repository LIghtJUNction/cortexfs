use crate::agent::stop::{
    StopError, execute_agent_cleanup, parse_runtime_stop_receipt, plan_agent_cleanup,
};
use crate::support::plain::{open_plain_directory, read_small_text_file};
use std::os::unix::fs::FileTypeExt;
use std::path::Path;

fn definition_active(root: &Path, name: &str) -> Result<bool, StopError> {
    let control = cortexfs_paths::agent_control_path(root, name);
    let socket = cortexfs_paths::agent_socket_path(root, name);
    let meta = read_small_text_file(
        &control.join("meta.json"),
        crate::agent::MAX_AGENT_RUNTIME_CONTROL_BYTES,
    )
    .map_err(|error| StopError::new(format!("cannot read runtime receipt: {error}")))?;
    let meta: serde_json::Value =
        serde_json::from_str(&meta).map_err(|_error| StopError::new("invalid runtime receipt"))?;
    if !meta.is_object() {
        return Err(StopError::new("invalid runtime receipt"));
    }
    if meta.get("runtime_receipt").is_none() {
        let socket = std::fs::symlink_metadata(socket)
            .map_err(|error| StopError::new(format!("cannot inspect agent socket: {error}")))?;
        return if socket.file_type().is_socket() {
            Ok(false)
        } else {
            Err(StopError::new("missing runtime receipt"))
        };
    }
    let runtime = parse_runtime_stop_receipt(&control)?;
    Ok(runtime.terminal_live || runtime.system_live)
}

pub fn remove_definition(root: &Path, name: &str, owner_uid: u32) -> Result<bool, StopError> {
    if !matches!(definition_active(root, name), Ok(false)) {
        return Ok(false);
    }
    let agent_root = cortexfs_paths::agent_root_path(root);
    let plan = plan_agent_cleanup(root, name, owner_uid)?;
    execute_agent_cleanup(plan)?;
    open_plain_directory(&agent_root)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| StopError::new(format!("cannot sync agent root: {error}")))?;
    Ok(true)
}
