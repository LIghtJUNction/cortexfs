use std::path::{Path, PathBuf};

use crate::CTX_ROOT;

#[must_use]
pub fn ctx_root() -> PathBuf {
    PathBuf::from(CTX_ROOT)
}

#[must_use]
pub fn status_path(root: &Path) -> PathBuf {
    root.join("status")
}

#[must_use]
pub fn bin_root_path(root: &Path) -> PathBuf {
    root.join("bin")
}

#[must_use]
pub fn model_root_path(root: &Path) -> PathBuf {
    root.join("model")
}

#[must_use]
pub fn agent_root_path(root: &Path) -> PathBuf {
    root.join("agent")
}

#[must_use]
pub fn tool_root_path(root: &Path) -> PathBuf {
    root.join("tool")
}

#[must_use]
pub fn home_root_path(root: &Path) -> PathBuf {
    root.join("home")
}

#[must_use]
pub fn shared_root_path(root: &Path) -> PathBuf {
    root.join("shared")
}

#[must_use]
pub fn root_entry_path(root: &Path, entry: &str) -> Option<PathBuf> {
    match entry {
        "status" => Some(status_path(root)),
        "bin" => Some(bin_root_path(root)),
        "model" => Some(model_root_path(root)),
        "agent" => Some(agent_root_path(root)),
        "tool" => Some(tool_root_path(root)),
        "home" => Some(home_root_path(root)),
        "shared" => Some(shared_root_path(root)),
        _ => None,
    }
}
