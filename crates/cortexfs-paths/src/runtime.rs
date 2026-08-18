use std::path::{Path, PathBuf};

use crate::CTX_ROOT;

pub const SYSTEM_RUNTIME_DIR: &str = "/run/cortexfs";
pub const SYSTEM_RUN_DIR: &str = "/run";
pub const SYSTEM_AGENT_RUNTIME_DIR: &str = "/run/cortexfs/agent";
pub const SYSTEM_CHANNEL_RUNTIME_DIR: &str = "/run/cortexfs/channel";
pub const SYSTEM_CONTROL_DIR: &str = "/run/cortexfs/control";
pub const AGENT_EXECUTABLE_SOCKET: &str = "/run/cortexfs/agent-executable";
pub const RUN_CONTROL_SOCKET: &str = "/run/cortexfs/control.sock";
pub const PROVIDER_EGRESS_SANDBOX_PATH: &str = "/run/cortexfs/provider-egress";

#[must_use]
pub fn system_agent_runtime_socket(agent: &str) -> PathBuf {
    Path::new(SYSTEM_AGENT_RUNTIME_DIR).join(format!("{agent}.sock"))
}

#[must_use]
pub fn channel_driver_socket(channel: &str) -> PathBuf {
    Path::new(SYSTEM_CHANNEL_RUNTIME_DIR).join(format!("{channel}.sock"))
}

#[must_use]
pub fn agent_client_socket(agent: &str) -> PathBuf {
    Path::new(CTX_ROOT)
        .join("agent")
        .join(format!("{agent}.sock"))
}

#[must_use]
pub fn agent_backing_socket(source: &Path, agent: &str) -> PathBuf {
    source.join("agent").join(format!("{agent}.sock"))
}

#[must_use]
pub fn system_agent_socket_unit(agent: &str) -> String {
    format!("cortexfs-agent@{agent}.socket")
}

#[must_use]
pub fn user_runtime_root(uid: u32) -> PathBuf {
    Path::new(SYSTEM_RUN_DIR).join("user").join(uid.to_string())
}

#[must_use]
pub fn user_systemd_transient_path(uid: &str, unit: &str) -> PathBuf {
    Path::new(SYSTEM_RUN_DIR)
        .join("user")
        .join(uid)
        .join("systemd")
        .join("transient")
        .join(unit)
}

#[must_use]
pub fn terminal_runtime_socket(runtime_root: &Path, agent: &str, session: &str) -> PathBuf {
    runtime_root
        .join("cortexfs")
        .join("terminal")
        .join(agent)
        .join(session)
        .join("main.sock")
}

#[must_use]
pub fn user_agent_runtime_socket(runtime_root: &Path, scope: &str, agent: &str) -> PathBuf {
    runtime_root
        .join("cortexfs")
        .join("agent")
        .join(scope)
        .join(format!("{agent}.sock"))
}

#[must_use]
pub fn run_control_dir() -> PathBuf {
    PathBuf::from(SYSTEM_CONTROL_DIR)
}

#[must_use]
pub fn system_run_root() -> PathBuf {
    PathBuf::from(SYSTEM_RUN_DIR)
}

#[must_use]
pub fn agent_executable_socket() -> PathBuf {
    PathBuf::from(AGENT_EXECUTABLE_SOCKET)
}
