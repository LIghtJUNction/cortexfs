use std::path::{Path, PathBuf};

use crate::shared_root_path;

#[must_use]
pub fn shared_path(root: &Path, name: &str) -> PathBuf {
    shared_root_path(root).join(name)
}

#[must_use]
pub fn shared_agent_path(root: &Path, space: &str, agent: &str) -> PathBuf {
    shared_agent_from_space_path(&shared_path(root, space), agent)
}

#[must_use]
pub fn shared_agent_from_space_path(space: &Path, agent: &str) -> PathBuf {
    shared_agent_root_from_space_path(space).join(agent)
}

#[must_use]
pub fn shared_agent_root_from_space_path(space: &Path) -> PathBuf {
    space.join("agent")
}

#[must_use]
pub fn shared_tool_path(root: &Path, space: &str, tool: &str) -> PathBuf {
    shared_tool_from_space_path(&shared_path(root, space), tool)
}

#[must_use]
pub fn shared_tool_from_space_path(space: &Path, tool: &str) -> PathBuf {
    space.join("tool").join(tool)
}
