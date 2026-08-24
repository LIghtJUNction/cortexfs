use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use super::{AgentLaunchError, AgentLaunchReceipt, SystemAgentSocketReceipt};
use crate::support::atomic::{
    atomic_create_text_with_mode, atomic_replace_text_preserving_metadata,
};

/// Persists receipt-bound cleanup evidence for one agent launch.
pub fn persist_agent_launch_meta(
    source: &Path,
    name: &str,
    terminal: &AgentLaunchReceipt,
    system: &SystemAgentSocketReceipt,
) -> Result<(), AgentLaunchError> {
    let control = cortexfs_paths::agent_control_path(source, name);
    let control_meta =
        fs::symlink_metadata(&control).map_err(|_error| AgentLaunchError::CannotExecute)?;
    if !control_meta.is_dir() || control_meta.file_type().is_symlink() {
        return Err(AgentLaunchError::CannotExecute);
    }
    let session = terminal
        .socket
        .parent()
        .and_then(Path::file_name)
        .and_then(std::ffi::OsStr::to_str)
        .filter(|session| crate::is_object_name(session))
        .ok_or(AgentLaunchError::CannotExecute)?;
    let meta_path = control.join("meta.json");
    let (mut meta, create) = match crate::support::plain::read_small_text_file(&meta_path, 65_536) {
        Ok(content) => {
            let meta = serde_json::from_str::<serde_json::Value>(&content)
                .ok()
                .and_then(|value| value.as_object().cloned())
                .ok_or(AgentLaunchError::CannotExecute)?;
            (meta, false)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (serde_json::Map::new(), true),
        Err(_error) => return Err(AgentLaunchError::CannotExecute),
    };
    meta.insert(
        "runtime_receipt".to_owned(),
        serde_json::json!({
            "version": 1,
            "control": { "dev": control_meta.dev(), "ino": control_meta.ino() },
            "terminal": {
                "session": session,
                "unit": terminal.unit,
                "invocation": terminal.invocation,
                "pid": terminal.pid,
                "identity": {
                    "uid": terminal.identity.uid(),
                    "gid": terminal.identity.gid(),
                    "groups": terminal.identity.groups(),
                }
            },
            "system": {
                "unit": system.unit,
                "invocation": system.invocation,
                "owned_start": system.owned_start,
            }
        }),
    );
    let encoded = serde_json::to_string(&meta).map_err(|_error| AgentLaunchError::CannotExecute)?;
    let content = format!("{encoded}\n");
    let recorded = if create {
        atomic_create_text_with_mode(&meta_path, &content, 0o644)
    } else {
        atomic_replace_text_preserving_metadata(&meta_path, &content)
    };
    recorded.map_err(|_error| AgentLaunchError::CannotExecute)?;
    let rebound =
        fs::symlink_metadata(&control).map_err(|_error| AgentLaunchError::CannotExecute)?;
    if (rebound.dev(), rebound.ino()) != (control_meta.dev(), control_meta.ino()) {
        return Err(AgentLaunchError::StopConflict);
    }
    Ok(())
}
