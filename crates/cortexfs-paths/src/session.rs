use std::path::{Path, PathBuf};

use crate::home_root_path;

#[must_use]
pub fn ctx_home_path(root: &Path, uid: &str) -> PathBuf {
    home_root_path(root).join(uid)
}

#[must_use]
pub fn home_agent_root_path(root: &Path, uid: &str) -> PathBuf {
    home_agent_root_from_home_path(&ctx_home_path(root, uid))
}

#[must_use]
pub fn home_agent_root_from_home_path(home: &Path) -> PathBuf {
    home.join("agent")
}

#[must_use]
pub fn home_tool_path(root: &Path, uid: &str) -> PathBuf {
    home_tool_from_home_path(&ctx_home_path(root, uid))
}

#[must_use]
pub fn home_tool_from_home_path(home: &Path) -> PathBuf {
    home.join("tool")
}

#[must_use]
pub fn home_model_path(root: &Path, uid: &str) -> PathBuf {
    home_model_from_home_path(&ctx_home_path(root, uid))
}

#[must_use]
pub fn home_model_from_home_path(home: &Path) -> PathBuf {
    home.join("model")
}

#[must_use]
pub fn agent_home_path(root: &Path, uid: &str, agent: &str) -> PathBuf {
    home_agent_root_path(root, uid).join(agent)
}

#[must_use]
pub fn agent_session_path(root: &Path, uid: &str, agent: &str, session: &str) -> PathBuf {
    agent_home_path(root, uid, agent)
        .join("session")
        .join(session)
}

#[must_use]
pub fn agent_sessions_path(root: &Path, uid: &str, agent: &str) -> PathBuf {
    agent_home_path(root, uid, agent).join("session")
}

#[must_use]
pub fn agent_sessions_from_home_path(home: &Path, agent: &str) -> PathBuf {
    home.join("agent").join(agent).join("session")
}

#[must_use]
pub fn session_terminal_from_home_path(
    home: &Path,
    agent: &str,
    session: &str,
    resource: &str,
) -> PathBuf {
    home.join("agent")
        .join(agent)
        .join("session")
        .join(session)
        .join("terminal")
        .join(resource)
}

#[must_use]
pub fn session_file_path(
    root: &Path,
    uid: &str,
    agent: &str,
    session: &str,
    file: &str,
) -> PathBuf {
    agent_session_path(root, uid, agent, session).join(file)
}

#[must_use]
pub fn session_index_file_path(session: &Path, file: &str) -> PathBuf {
    session.join("index").join(file)
}

#[must_use]
pub fn session_terminal_path(
    root: &Path,
    uid: &str,
    agent: &str,
    session: &str,
    resource: &str,
) -> PathBuf {
    agent_session_path(root, uid, agent, session)
        .join("terminal")
        .join(resource)
}
