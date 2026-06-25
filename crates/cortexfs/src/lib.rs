//! `CortexFS` Agent OS ABI design core.
//!
//! The old CLI, daemon, provider registry, and FUSE projection were removed
//! before the Agent OS rewrite. This crate intentionally exposes only stable
//! ABI names while the implementation is redesigned around Rig.

use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{
    DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt, symlink,
};
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use nix::libc;
use nix::sys::socket::{getsockopt, sockopt};
use serde::Deserialize;
use serde_json::Value;

macro_rules! impl_issue_report {
    ($report:ty, $issue:ty) => {
        impl $report {
            #[must_use]
            pub const fn new(issues: Vec<$issue>) -> Self {
                Self { issues }
            }

            #[must_use]
            pub fn is_ok(&self) -> bool {
                self.issues.is_empty()
            }

            #[must_use]
            pub fn issues(&self) -> &[$issue] {
                &self.issues
            }
        }
    };
}

mod abi_constants;
mod abi_path;
mod abi_path_parse;
mod agent_control;
mod context_jsonl;
mod context_pack;
mod context_pack_build;
mod context_pack_inspect;
mod context_pack_source;
mod core_tools;
mod message_stream;
mod model;
mod mount_table;
mod policy;
mod session_index;
mod session_layout;
mod shared_queue;
mod socket_request;
mod stream;
mod tool_path;
mod tool_schema;

pub use abi_constants::{
    AGENT_CONTROL_FILES, CHILD_RESULT_REQUIRED_DIRS, CHILD_RESULT_REQUIRED_FILES,
    CONTEXT_REQUIRED_DIRS, CONTEXT_REQUIRED_FILES, CORTEXFS_OBJECT_RUNNER, CTX_ROOT,
    DEFAULT_AGENT_PROMPT_TEMPLATE, EXEC_OBJECTS, FORBIDDEN_MODEL_CAPABILITIES, FUSE_V1_ROOT_INODE,
    MAX_FUSE_V1_SMALL_WRITE_BYTES, MAX_OBJECT_NAME_LEN, MAX_SOCKET_FRAME_BYTES,
    MODEL_CONTROL_FILES, ROOT_ENTRIES, SESSION_REQUIRED_FILES, SHARED_QUEUE_REQUIRED_DIRS,
    STABLE_MODEL_CAPABILITIES, TOOL_CONTROL_FILES,
};
use abi_constants::{
    DEBUG_ECHO_MODEL, DEBUG_ECHO_NAME, DEBUG_ECHO_PROVIDER, DEFAULT_MODEL_ALIAS,
    DEFAULT_MODEL_ALIAS_TARGET, HELPER_MODEL_ALIAS, SYSTEM_PROVIDER_CONFIG_DIR,
    SYSTEM_PROVIDER_MODEL_CACHE_DIR,
};
use abi_path::is_object_name_for_class;
pub use abi_path::{
    AbiPathKind, ObjectClass, classify_abi_path, is_model_name, is_object_name, is_root_entry,
    parse_abi_path,
};
pub use agent_control::{
    AgentControlIssue, AgentControlKind, AgentControlReport, inspect_agent_control,
};
pub use context_pack::{
    ContextPackBuild, ContextPackBuildError, ContextPackBuiltItem, ContextPackIssue,
    ContextPackReport, ContextPackSourceError, inspect_context_pack_json, rebuild_context_pack,
    validate_context_pack_source,
};
pub use core_tools::{
    FsReadTool, FsWriteTool, ShellExecTool, TshConfigTool, core_tool_specs, run_core_tool,
    run_core_tool_cli,
};
pub use model::{
    Capability, ModelCapabilities, ModelCapabilityIssue, ModelCapabilityRegistry,
    ModelCapabilityReport, ModelDriverRouteError, ModelDriverRoutingTable, ModelDriverUseCase,
    ModelRegistryError, inspect_model_capabilities, parse_model_driver_routes,
};
pub use mount_table::{MountEntry, MountError, MountMode, MountOption, MountTable};
pub use policy::{PolicyError, PolicyObjectClass, PolicyPermission, PolicyRule, PolicyV0};
pub use session_index::{
    SessionIndexIssue, SessionIndexKind, SessionIndexReport, SessionIndexUpdateError,
    inspect_session_index, update_session_index,
};
pub use session_layout::{
    SessionControlIssue, SessionControlKind, SessionControlReport, SessionLayoutIssue,
    SessionLayoutReport, inspect_session_control, inspect_session_layout,
};
pub use shared_queue::{
    SharedQueueClaim, SharedQueueClaimError, SharedQueueFinishError, SharedQueueLayoutIssue,
    SharedQueueLayoutReport, SharedQueueOutcome, SharedQueueRecoverError,
    claim_next_shared_queue_job, finish_shared_queue_job, inspect_shared_queue_layout,
    recover_shared_queue_job,
};
pub use socket_request::{
    SocketRequest, SocketRequestError, SocketSessionScope, parse_socket_request_frame,
};
pub use stream::{
    ContextJsonlIssue, ContextJsonlKind, ContextJsonlReport, EventStreamIssue, EventStreamReport,
    MessageStreamIssue, MessageStreamReport, inspect_context_jsonl, inspect_event_stream_jsonl,
    inspect_message_stream_jsonl,
};
pub use tool_path::{ToolHit, ToolPath, ToolPathError, is_executable_file};
pub use tool_schema::{ToolSchemaIssue, ToolSchemaReport, inspect_tool_schema_json};

include!("fuse_v1_types.rs");

include!("core_runtime_types.rs");

include!("agent_runtime_types.rs");

include!("authority_types.rs");

include!("child_agent_types.rs");

impl_issue_report!(ObjectLayoutReport, ObjectLayoutIssue);

impl ObjectBootstrap {
    /// Creates a bootstrap result.
    #[must_use]
    pub const fn new(executable: PathBuf, control_dir: PathBuf) -> Self {
        Self {
            executable,
            control_dir,
        }
    }

    /// Returns the executable entry path.
    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// Returns the `.d` control directory path.
    #[must_use]
    pub fn control_dir(&self) -> &Path {
        &self.control_dir
    }
}

include!("fuse_v1_projection.rs");

include!("fuse_v1_model_alias.rs");

impl ReferenceTreeBootstrap {
    /// Creates a reference-tree bootstrap result.
    #[must_use]
    pub const fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Returns the root that was materialized.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }
}

include!("fuse_v1_provider.rs");

include!("provider_model_discovery.rs");

impl ObjectBootstrapError {
    /// Returns a stable errno name for this object bootstrap failure.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidObjectName
            | Self::InvalidWrapperTarget
            | Self::InvalidControlFile
            | Self::InvalidControlValue => "EINVAL",
            Self::CannotCreate | Self::CannotRecord | Self::CannotChmod => "EIO",
        }
    }
}

/// Reads Linux peer credentials from a connected Unix socket.
pub fn peer_credentials(stream: &UnixStream) -> Result<PeerCredentials, PeerCredentialError> {
    getsockopt(stream, sockopt::PeerCredentials)
        .map(|credentials| {
            PeerCredentials::new(
                Some(credentials.pid()),
                credentials.uid(),
                credentials.gid(),
            )
        })
        .map_err(|_error| PeerCredentialError::CannotRead)
}

/// Ensures `session_root/<session>/` has the durable v1 session layout.
///
/// This creates only documented session files, context directories, and the
/// reserved `session/index` files. Existing files are preserved; the helper
/// does not start a model, run an agent, or synthesize hidden history.
pub fn ensure_durable_session_layout(
    session_root: &Path,
    session_name: &str,
    cwd: &str,
    model: Option<&str>,
    scope: SocketSessionScope,
) -> Result<(), DurableSessionLayoutError> {
    if !is_object_name(session_name) {
        return Err(DurableSessionLayoutError::InvalidSessionName);
    }
    if !is_stable_chroot_absolute_path(cwd) {
        return Err(DurableSessionLayoutError::InvalidCwd);
    }
    if let Some(model) = model
        && !abi_path::is_model_reference(model)
    {
        return Err(DurableSessionLayoutError::InvalidModelName);
    }
    if scope == SocketSessionScope::Temp {
        return Err(DurableSessionLayoutError::TempSessionNotDurable);
    }

    let session_dir = session_root.join(session_name);
    let context = session_dir.join("context");
    create_dir(session_root)?;
    create_dir(&session_dir)?;
    create_dir(&context)?;
    for dir in CONTEXT_REQUIRED_DIRS {
        create_dir(&context.join(dir))?;
    }
    create_dir(&context.join("swap").join("chunk"))?;
    create_dir(&context.join("dedup").join("blob"))?;
    let index = session_root.join("index");
    create_dir(&index)?;
    create_dir(&index.join("by-cwd"))?;

    let now = unix_timestamp_text();
    write_text_file_if_missing(&session_dir.join("messages.jsonl"), "")?;
    write_text_file_if_missing(&session_dir.join("events.jsonl"), "")?;
    write_text_file_if_missing(&session_dir.join("latest.md"), "")?;
    write_text_file_if_missing(&session_dir.join("state"), "idle\n")?;
    write_text_file_if_missing(&session_dir.join("cwd"), &format!("{cwd}\n"))?;
    write_text_file_if_missing(&session_dir.join("created_at"), &now)?;
    write_text_file_if_missing(&session_dir.join("updated_at"), &now)?;
    write_text_file(
        &session_dir.join("meta.json"),
        &durable_session_meta_json(model, scope),
    )?;

    write_text_file_if_missing(&context.join("budget"), "0\n")?;
    write_text_file_if_missing(
        &context.join("pack.json"),
        &format!(
            "{}\n",
            serde_json::json!({
                "session": session_name,
                "items": []
            })
        ),
    )?;
    write_text_file_if_missing(&context.join("pack.md"), "")?;
    write_text_file_if_missing(&context.join("summary.md"), "")?;
    write_text_file_if_missing(&context.join("facts.jsonl"), "")?;
    write_text_file_if_missing(&context.join("decisions.jsonl"), "")?;
    write_text_file_if_missing(&context.join("todo.md"), "")?;
    write_text_file_if_missing(&context.join("refs.jsonl"), "")?;
    write_text_file_if_missing(&context.join("swap").join("index.jsonl"), "")?;
    write_text_file_if_missing(&context.join("dedup").join("index.jsonl"), "")?;

    write_text_file_if_missing(
        &session_root.join("index").join("list"),
        &format!("{session_name}\n"),
    )?;
    write_text_file_if_missing(
        &session_root.join("index").join("current"),
        &format!("{session_name}\n"),
    )?;

    Ok(())
}

fn durable_session_meta_json(model: Option<&str>, scope: SocketSessionScope) -> String {
    let mut value = serde_json::json!({
        "client": "ctx",
        "scope": scope.as_str()
    });
    if let Some(model) = model
        && let Some(object) = value.as_object_mut()
    {
        object.insert("model".to_owned(), serde_json::json!(model));
    }
    format!("{value}\n")
}

fn create_dir(path: &Path) -> Result<(), DurableSessionLayoutError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(DurableSessionLayoutError::CannotCreate);
        }
        return set_private_dir_permissions(path);
    }
    let mut missing = Vec::new();
    let mut cursor = Some(path);
    while let Some(current) = cursor {
        match fs::symlink_metadata(current) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(DurableSessionLayoutError::CannotCreate);
            }
            Ok(_) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(current.to_path_buf());
                cursor = current.parent();
            }
            Err(_) => return Err(DurableSessionLayoutError::CannotCreate),
        }
    }
    for dir in missing.iter().rev() {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
        set_private_dir_permissions(dir)?;
    }
    Ok(())
}

fn write_text_file_if_missing(path: &Path, content: &str) -> Result<(), DurableSessionLayoutError> {
    if path.exists() {
        return if path.is_file() {
            set_text_file_permissions(path)
        } else {
            Err(DurableSessionLayoutError::CannotCreate)
        };
    }
    if let Some(parent) = path.parent() {
        create_dir(parent)?;
    }
    write_private_text_file(path, content)?;
    set_text_file_permissions(path)
}

fn write_text_file(path: &Path, content: &str) -> Result<(), DurableSessionLayoutError> {
    if let Some(parent) = path.parent() {
        create_dir(parent)?;
    }
    write_private_text_file(path, content)?;
    set_text_file_permissions(path)
}

fn set_text_file_permissions(path: &Path) -> Result<(), DurableSessionLayoutError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(DurableSessionLayoutError::CannotCreate);
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)
}

fn set_private_dir_permissions(path: &Path) -> Result<(), DurableSessionLayoutError> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)
}

fn write_private_text_file(path: &Path, content: &str) -> Result<(), DurableSessionLayoutError> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    file.write_all(content.as_bytes())
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    file.flush()
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)
}

include!("socket_runtime.rs");

include!("socket_session_record.rs");

include!("agent_runtime_view.rs");

include!("object_metadata.rs");

include!("object_bootstrap.rs");

include!("reference_tree_bootstrap.rs");

include!("reference_tree_helpers.rs");

include!("fuse_v1_path.rs");

fn shell_single_quote(value: &str) -> String {
    let mut quoted = String::from("'");
    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}

fn set_executable_mode(path: &Path) -> Result<(), ObjectBootstrapError> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
        .map_err(|_error| ObjectBootstrapError::CannotChmod)
}

include!("object_layout.rs");

include!("authority.rs");

include!("authority_helpers.rs");

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/lib_tests.rs"
    ));
}
