#![forbid(unsafe_code)]
#![expect(
    clippy::allow_attributes,
    reason = "allow compiler lint unfulfilled_lint_expectations"
)]
#![allow(
    unfulfilled_lint_expectations,
    reason = "expected target-specific lint results"
)]
#![expect(
    clippy::wildcard_imports,
    reason = "uniform submodules with wildcard imports"
)]
#![expect(
    ambiguous_glob_reexports,
    reason = "private core modules overlap in globs"
)]
#![expect(
    clippy::field_scoped_visibility_modifiers,
    reason = "internal structs with scoped fields"
)]

//! `CortexFS` ABI design core.

#[path = "imports.rs"]
pub mod imports;
pub use imports::*;

const MAX_AGENT_STDOUT_QUEUE_FRAMES: usize = 16;

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

pub mod abi;
pub mod agent;
pub mod authority;
pub mod context;
pub mod mount;
pub mod policy;
pub mod provider;
pub mod tool;

pub mod fuse;
pub mod object;
pub mod reference;
pub mod runtime;
pub mod support;

// Re-export moved modules at the root so that existing code referencing them as crate::module works unchanged.
pub use abi::authority_types;
pub use abi::authority_types::*;
pub use abi::socket_request;
pub use abi::socket_request::*;
pub use policy::subject as policy_subject;
pub use provider::model_discovery::*;
pub use support::control_text::ControlLineIssue;
pub use support::jsonl_line::{JsonlLineShape, for_each_jsonl_line, parse_jsonl_line};
pub use support::layout_path::{LayoutPathRole, PathLayoutIssue};
pub use support::{
    ATIF_SCHEMA_VERSION, MAX_TRAJECTORY_SESSION_FILE_BYTES, TRAJECTORY_DEFAULT_AGENT_NAME,
    Trajectory, TrajectoryAgent, TrajectoryFinalMetrics, TrajectoryIssue, TrajectoryMapError,
    TrajectoryMetrics, TrajectoryObservation, TrajectoryObservationResult, TrajectoryReport,
    TrajectoryStep, TrajectoryToolCall, control_text, host_path, jsonl_line, layout_path, manuals,
    message_stream, plain_fs, process_helpers, session_index, session_layout, shared_queue, stream,
    tool_path, tool_schema, trajectory, trajectory_from_session_dir, trajectory_from_session_jsonl,
    validate_trajectory, write_trajectory_json,
};
pub use tool::core::runtime_types::*;
pub use tool::tsh_context_state;

pub use authority::helpers as authority_helpers;
pub use authority::*;

// Re-export contents of FUSE and runtime etc. since they were previously included in lib.rs
pub(crate) use fuse::v1_path::*;
pub use fuse::v1_projection::*;
pub(crate) use fuse::v1_provider::*;
pub use fuse::v1_types::*;

pub use runtime::socket::*;
pub use runtime::socket_session_record::*;
pub use runtime::socket_types::*;

pub use object::bootstrap::*;
pub use object::layout::*;
pub use object::metadata::*;

pub use reference::tree_bootstrap::*;
pub use reference::tree_helpers::*;

pub use agent::child_types::*;
pub use agent::runtime_types::*;
pub use agent::runtime_view::*;

#[cfg(test)]
pub(crate) use agent::secret_resolution::*;

pub(crate) use authority::helpers::*;
pub use policy::subject::*;

use abi::constants::{
    DEBUG_ECHO_MODEL, DEBUG_ECHO_NAME, DEBUG_ECHO_PROVIDER, DEFAULT_MODEL_ALIAS,
    DEFAULT_MODEL_ALIAS_TARGET, DEFAULT_MODEL_ROUTE, HELPER_MODEL_ALIAS, HELPER_MODEL_ALIAS_TARGET,
    MODEL_ROUTE_FILE, SYSTEM_PROVIDER_CONFIG_DIR, SYSTEM_PROVIDER_MODEL_CACHE_DIR,
};
use abi::path::is_object_name_for_class;
use plain_fs::{
    open_plain_directory as open_session_layout_plain_directory,
    plain_file_name as session_layout_plain_file_name,
};

#[path = "exports.rs"]
pub mod exports;
pub use exports::*;

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
        && !abi::path::is_model_reference(model)
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
    create_dir(&index.join("by-hash"))?;
    create_dir(&index.join("by-uuid"))?;

    let now = unix_timestamp_text();
    write_text_file_if_missing(&session_dir.join("messages.jsonl"), "")?;
    write_text_file_if_missing(&session_dir.join("events.jsonl"), "")?;
    write_text_file_if_missing(&session_dir.join("latest.md"), "")?;
    write_text_file_if_missing(&session_dir.join("state"), "idle\n")?;
    write_text_file_if_missing(&session_dir.join("cwd"), &format!("{cwd}\n"))?;
    write_text_file_if_missing(&session_dir.join("created_at"), &now)?;
    write_text_file_if_missing(&session_dir.join("updated_at"), &now)?;
    let meta_json = session_dir.join("meta.json");
    write_text_file_if_missing(&meta_json, &durable_session_meta_json(model, scope))?;

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

pub(crate) fn durable_session_meta_json(model: Option<&str>, scope: SocketSessionScope) -> String {
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

pub(crate) fn create_dir(path: &Path) -> Result<(), DurableSessionLayoutError> {
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
    let parent = missing
        .last()
        .and_then(|path| path.parent())
        .ok_or(DurableSessionLayoutError::CannotCreate)?;
    let mut parent_dir = open_session_layout_plain_directory(parent)
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    for dir in missing.iter().rev() {
        let name = session_layout_plain_file_name(dir)
            .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
        nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(0o700),
        )
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
        parent_dir
            .sync_all()
            .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
        let child = nix::fcntl::openat(
            &parent_dir,
            name,
            nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
        parent_dir = fs::File::from(child);
        parent_dir
            .set_permissions(fs::Permissions::from_mode(0o700))
            .and_then(|()| parent_dir.sync_all())
            .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    }
    Ok(())
}

pub(crate) fn write_text_file_if_missing(
    path: &Path,
    content: &str,
) -> Result<(), DurableSessionLayoutError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.is_file() {
            set_text_file_permissions(path)
        } else {
            Err(DurableSessionLayoutError::CannotCreate)
        };
    }
    if let Some(parent) = path.parent() {
        create_dir(parent)?;
    }
    write_private_text_file(path, content)
}

pub(crate) fn set_text_file_permissions(path: &Path) -> Result<(), DurableSessionLayoutError> {
    let (_parent_dir, file) = open_session_layout_file_at(
        path,
        nix::fcntl::OFlag::O_RDWR | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )?;
    if !file
        .metadata()
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?
        .is_file()
    {
        return Err(DurableSessionLayoutError::CannotCreate);
    }
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .and_then(|()| file.sync_all())
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)
}

pub(crate) fn set_private_dir_permissions(path: &Path) -> Result<(), DurableSessionLayoutError> {
    let dir = open_session_layout_plain_directory(path)
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    dir.set_permissions(fs::Permissions::from_mode(0o700))
        .and_then(|()| dir.sync_all())
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)
}

pub(crate) fn write_private_text_file(
    path: &Path,
    content: &str,
) -> Result<(), DurableSessionLayoutError> {
    let (parent_dir, mut file) = open_session_layout_file_at(
        path,
        nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_TRUNC
            | nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    )?;
    file.write_all(content.as_bytes())
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all())
        .and_then(|()| parent_dir.sync_all())
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)
}

pub(crate) fn open_session_layout_file_at(
    path: &Path,
    flags: nix::fcntl::OFlag,
    mode: nix::sys::stat::Mode,
) -> Result<(fs::File, fs::File), DurableSessionLayoutError> {
    let parent = path
        .parent()
        .ok_or(DurableSessionLayoutError::CannotCreate)?;
    let file_name = session_layout_plain_file_name(path)
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    let parent_dir = open_session_layout_plain_directory(parent)
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    let file_fd = nix::fcntl::openat(&parent_dir, file_name, flags, mode)
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    Ok((parent_dir, fs::File::from(file_fd)))
}

pub(crate) fn shell_single_quote(value: &str) -> String {
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

pub(crate) fn set_executable_mode(path: &Path) -> Result<(), ObjectBootstrapError> {
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_error| ObjectBootstrapError::CannotChmod)?;
    if !file
        .metadata()
        .map_err(|_error| ObjectBootstrapError::CannotChmod)?
        .is_file()
    {
        return Err(ObjectBootstrapError::CannotChmod);
    }
    file.set_permissions(fs::Permissions::from_mode(0o755))
        .and_then(|()| file.sync_all())
        .map_err(|_error| ObjectBootstrapError::CannotChmod)
}

#[cfg(test)]
#[expect(unused_qualifications, reason = "tests use qualified paths")]
mod tests;
