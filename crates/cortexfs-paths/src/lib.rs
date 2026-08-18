#![forbid(unsafe_code)]

//! Stable `CortexFS` filesystem, runtime, and host path ABI.
//!
//! The crate only composes paths. Callers must validate dynamic components
//! with [`validate_component`] before inserting untrusted values.

mod host;
mod object;
mod root;
mod runtime;
mod session;
mod shared;

pub use host::{
    SYSTEM_AGENT_PROMPT_PATH, SYSTEM_CHANNEL_CONFIG_DIR, SYSTEM_PROVIDER_CONFIG_DIR,
    SYSTEM_PROVIDER_MODEL_CACHE_DIR, SYSTEM_PROVIDER_SECRET_DIR, SYSTEM_STORAGE_CURRENT,
    SYSTEM_STORAGE_DIR, channel_config_path, provider_config_path, provider_model_cache_path,
    provider_secret_path, provider_secret_root_path, storage_current_link_path,
    storage_current_path, storage_generation_path, storage_generations_path, storage_root_path,
    storage_update_lock_path,
};
pub use object::{
    agent_control_file_path, agent_control_path, agent_path, agent_socket_path, control_file_path,
    model_control_file_path, model_control_path, model_path, model_provider_path,
    model_reference_path, model_route_path, model_socket_path, object_control_file_path,
    object_control_path, object_path, object_root_path, object_runner_path, object_socket_path,
    tool_config_path, tool_control_file_path, tool_control_path, tool_path,
};
pub use root::{
    agent_root_path, bin_root_path, ctx_root, home_root_path, model_root_path, root_entry_path,
    shared_root_path, status_path, tool_root_path,
};
pub use runtime::{
    AGENT_EXECUTABLE_SOCKET, PROVIDER_EGRESS_SANDBOX_PATH, RUN_CONTROL_SOCKET,
    SYSTEM_AGENT_RUNTIME_DIR, SYSTEM_CHANNEL_RUNTIME_DIR, SYSTEM_CONTROL_DIR, SYSTEM_RUN_DIR,
    SYSTEM_RUNTIME_DIR, agent_backing_socket, agent_client_socket, agent_executable_socket,
    channel_driver_socket, run_control_dir, system_agent_runtime_socket, system_agent_socket_unit,
    system_run_root, terminal_runtime_socket, user_agent_runtime_socket, user_runtime_root,
    user_systemd_transient_path,
};
pub use session::{
    agent_home_path, agent_session_path, agent_sessions_from_home_path, agent_sessions_path,
    ctx_home_path, home_agent_root_from_home_path, home_agent_root_path, home_model_from_home_path,
    home_model_path, home_tool_from_home_path, home_tool_path, session_file_path,
    session_index_file_path, session_terminal_from_home_path, session_terminal_path,
};
pub use shared::{
    shared_agent_from_space_path, shared_agent_path, shared_agent_root_from_space_path,
    shared_path, shared_tool_from_space_path, shared_tool_path,
};

/// Canonical `/ctx` root for the system mount.
pub const CTX_ROOT: &str = "/ctx";
/// Stable root entries exposed by the FUSE ABI.
pub const ROOT_ENTRIES: &[&str] = &["status", "bin", "model", "agent", "tool", "home", "shared"];
/// Maximum length of a single dynamic path component.
pub const MAX_COMPONENT_LEN: usize = 64;
/// Canonical executable used by projected object files.
pub const CORTEXFS_OBJECT_RUNNER: &str = "/ctx/bin/cortexfs-object-runner";

/// A dynamic path component that cannot be safely composed into an ABI path.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathComponentError {
    Empty,
    TooLong,
    Dot,
    Separator,
    Nul,
}

impl std::fmt::Display for PathComponentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match *self {
            Self::Empty => "path component is empty",
            Self::TooLong => "path component is too long",
            Self::Dot => "path component is dot-like",
            Self::Separator => "path component contains a separator",
            Self::Nul => "path component contains NUL",
        };
        f.write_str(message)
    }
}

impl std::error::Error for PathComponentError {}

/// Validates one dynamic component before it is inserted into an ABI path.
pub fn validate_component(value: &str) -> Result<(), PathComponentError> {
    if value.is_empty() {
        return Err(PathComponentError::Empty);
    }
    if value.len() > MAX_COMPONENT_LEN {
        return Err(PathComponentError::TooLong);
    }
    if value == "." || value == ".." {
        return Err(PathComponentError::Dot);
    }
    if value.contains('/') || value.contains('\\') {
        return Err(PathComponentError::Separator);
    }
    if value.contains('\0') {
        return Err(PathComponentError::Nul);
    }
    Ok(())
}

/// Returns whether `value` is safe to use as one path component.
#[must_use]
pub fn is_component(value: &str) -> bool {
    validate_component(value).is_ok()
}
