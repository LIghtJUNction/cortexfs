#![forbid(unsafe_code)]

//! `CortexFS` Agent OS ABI design core.
//!
//! The old CLI, daemon, provider registry, and FUSE projection were removed
//! before the Agent OS rewrite. This crate intentionally exposes only stable
//! ABI names while the implementation is redesigned around Rig.

include!("source_imports.rs");

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

include!("source_modules.rs");

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

include!("public_exports.rs");

include!("fuse/v1_types.rs");

include!("tool/core/runtime_types.rs");

include!("agent/runtime_types.rs");

include!("runtime/socket_types.rs");

include!("authority_types.rs");

include!("agent/child_types.rs");

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

include!("fuse/v1_projection.rs");

include!("fuse/v1_model_alias.rs");

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

include!("fuse/v1_provider.rs");

include!("provider/model_discovery.rs");

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
    write_private_text_file(&meta_json, &durable_session_meta_json(model, scope))?;

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

fn write_text_file_if_missing(path: &Path, content: &str) -> Result<(), DurableSessionLayoutError> {
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

fn set_text_file_permissions(path: &Path) -> Result<(), DurableSessionLayoutError> {
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

fn set_private_dir_permissions(path: &Path) -> Result<(), DurableSessionLayoutError> {
    let dir = open_session_layout_plain_directory(path)
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    dir.set_permissions(fs::Permissions::from_mode(0o700))
        .and_then(|()| dir.sync_all())
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)
}

fn write_private_text_file(path: &Path, content: &str) -> Result<(), DurableSessionLayoutError> {
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

fn open_session_layout_file_at(
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

include!("runtime/socket.rs");

include!("runtime/socket_session_record.rs");

include!("policy_subject.rs");

include!("agent/runtime_view.rs");

include!("object/metadata.rs");

include!("object/bootstrap.rs");

include!("reference/tree_bootstrap.rs");

include!("reference/tree_helpers.rs");

include!("fuse/v1_path.rs");

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

include!("object/layout.rs");

include!("authority.rs");

include!("authority_helpers.rs");

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/lib_tests.rs"
    ));
}
