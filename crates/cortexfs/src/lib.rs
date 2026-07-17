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

extern crate self as cortexfs;

pub mod imports;
pub use imports::*;

#[doc(hidden)]
pub mod cli;

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

// Module layers (docs/internal-architecture.md §4): depend only downward.
// L0 abi/policy → L1 support → L2 authority/context → L3 provider/tool →
// L4 reference/mount → L5 agent/runtime → L6 object → L7 fuse → L8 bin/*.

/// L0 — pure ABI grammar and request types.
pub mod abi;
/// L5 — agent lifecycle, launch, child, schedule (not FUSE projection).
pub mod agent;
/// L2 — identity and authority helpers.
pub mod authority;
/// L2 — context packs and working-set construction.
pub mod context;
/// L4 — mount table types and parsing.
pub mod mount;
/// L0 — SELinux-like allowlist policy types.
pub mod policy;
/// L3 — provider/model registry (not root ABI).
pub mod provider;
/// L3 — tool schema and execution authority helpers.
pub mod tool;

/// L7 — FUSE projection only; must not call `object::executor`.
pub mod fuse;
/// L6 — object install/swap and one-shot executor/runner.
pub mod object;
/// L4 — storage generations and bootstrap tree.
pub mod reference;
/// L5 — sockets, durable session record, egress.
pub mod runtime;
/// L1 — plain files, jsonl, layout, process helpers (no FUSE/HTTP).
pub mod support;

pub use abi::authority::*;
pub use abi::request::*;
pub use provider::discovery::*;
pub use support::control::ControlLineIssue;
pub use support::jsonl::{JsonlLineShape, for_each_jsonl_line, parse_jsonl_line};
pub use support::layout::{LayoutPathRole, PathLayoutIssue};
pub use support::{
    ATIF_SCHEMA_VERSION, MAX_TRAJECTORY_SESSION_FILE_BYTES, TRAJECTORY_DEFAULT_AGENT_NAME,
    Trajectory, TrajectoryAgent, TrajectoryFinalMetrics, TrajectoryIssue, TrajectoryMapError,
    TrajectoryMetrics, TrajectoryObservation, TrajectoryObservationResult, TrajectoryReport,
    TrajectoryStep, TrajectoryToolCall, columnar, manuals, stream, trajectory,
    trajectory_from_session_dir, trajectory_from_session_jsonl, validate_trajectory,
    write_trajectory_json,
};
pub use tool::core::runtime::*;

pub use authority::*;

// Re-export contents of FUSE and runtime etc. since they were previously included in lib.rs
pub(crate) use fuse::path::*;
pub use fuse::projection::*;
pub(crate) use fuse::provider::*;
pub use fuse::types::*;

pub use runtime::record::*;
pub use runtime::socket::*;
pub use runtime::types::*;

pub use object::bootstrap::*;
pub use object::layout::*;
pub use object::metadata::*;

pub use reference::bootstrap::*;
pub use reference::helpers::*;

pub use agent::child::*;
pub use agent::launch::{
    AgentLaunchCommand, AgentLaunchRequest, chat_socket_command, invocation_id, launch_process_for,
    parse_main_pid, reset_unit_for, set_user_systemd_client_env, unit_main_pid_for,
};
pub use agent::runtime::*;
pub use agent::view::*;

#[cfg(test)]
pub(crate) use agent::secret::*;

pub(crate) use authority::helpers::*;
pub use policy::subject::*;

use abi::constants::{
    DEBUG_ECHO_MODEL, DEBUG_ECHO_NAME, DEBUG_ECHO_PROVIDER, DEFAULT_MODEL_ALIAS,
    DEFAULT_MODEL_ALIAS_TARGET, DEFAULT_MODEL_ROUTE, HELPER_MODEL_ALIAS, HELPER_MODEL_ALIAS_TARGET,
    MODEL_ROUTE_FILE, SYSTEM_PROVIDER_CONFIG_DIR, SYSTEM_PROVIDER_MODEL_CACHE_DIR,
};
use abi::path::is_object_name_for_class;
use support::plain::{open_plain_directory, plain_file_name};

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

/// Ensures `session_root/<session>/` has the durable session layout.
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
) -> Result<SessionLayoutReceipts, DurableSessionLayoutError> {
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

    let mut receipts = SessionLayoutReceipts::default();
    let result = (|| {
        let session_dir = session_root.join(session_name);
        let context = session_dir.join("context");
        create_dir(session_root, &mut receipts)?;
        create_dir(&session_dir, &mut receipts)?;
        create_dir(&context, &mut receipts)?;
        for dir in CONTEXT_REQUIRED_DIRS {
            create_dir(&context.join(dir), &mut receipts)?;
        }
        create_dir(&context.join("swap").join("chunk"), &mut receipts)?;
        create_dir(&context.join("dedup").join("blob"), &mut receipts)?;
        let index = session_root.join("index");
        create_dir(&index, &mut receipts)?;
        create_dir(&index.join("by-cwd"), &mut receipts)?;
        create_dir(&index.join("by-hash"), &mut receipts)?;
        create_dir(&index.join("by-uuid"), &mut receipts)?;

        let now = unix_timestamp_text();
        record_text(&session_dir.join("messages.jsonl"), "", &mut receipts)?;
        record_text(&session_dir.join("events.jsonl"), "", &mut receipts)?;
        record_text(&session_dir.join("latest.md"), "", &mut receipts)?;
        record_text(&session_dir.join("state"), "idle\n", &mut receipts)?;
        record_text(&session_dir.join("cwd"), &format!("{cwd}\n"), &mut receipts)?;
        record_text(&session_dir.join("created_at"), &now, &mut receipts)?;
        record_text(&session_dir.join("updated_at"), &now, &mut receipts)?;
        let meta_json = session_dir.join("meta.json");
        record_text(
            &meta_json,
            &durable_session_meta_json(model, scope),
            &mut receipts,
        )?;

        record_text(&context.join("budget"), "0\n", &mut receipts)?;
        record_text(
            &context.join("pack.json"),
            &format!(
                "{}\n",
                serde_json::json!({
                    "session": session_name,
                    "items": []
                })
            ),
            &mut receipts,
        )?;
        for path in [
            context.join("pack.md"),
            context.join("summary.md"),
            context.join("facts.jsonl"),
            context.join("decisions.jsonl"),
            context.join("todo.md"),
            context.join("refs.jsonl"),
            context.join("swap/index.jsonl"),
            context.join("dedup/index.jsonl"),
        ] {
            record_text(&path, "", &mut receipts)?;
        }

        record_text(
            &session_root.join("index").join("list"),
            &format!("{session_name}\n"),
            &mut receipts,
        )?;
        record_text(
            &session_root.join("index").join("current"),
            &format!("{session_name}\n"),
            &mut receipts,
        )?;

        Ok::<(), DurableSessionLayoutError>(())
    })();
    if let Err(error) = result {
        let _rollback = agent::create::rollback_session_layout(receipts);
        return Err(error);
    }
    Ok(receipts)
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct SessionLayoutReceipt {
    pub(crate) path: PathBuf,
    pub(crate) dev: u64,
    pub(crate) ino: u64,
    pub(crate) directory: bool,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct SessionLayoutReceipts {
    entries: Vec<SessionLayoutReceipt>,
}

impl SessionLayoutReceipts {
    pub(crate) fn into_entries(self) -> Vec<SessionLayoutReceipt> {
        self.entries
    }
}

#[cfg(test)]
type SessionLayoutHook = Box<dyn FnOnce(&Path)>;

#[cfg(test)]
thread_local! {
    static SESSION_LAYOUT_FILE_RACE: std::cell::RefCell<Option<SessionLayoutHook>> =
        const { std::cell::RefCell::new(None) };
    static SESSION_LAYOUT_DIR_FAULT: std::cell::RefCell<Option<SessionLayoutHook>> =
        const { std::cell::RefCell::new(None) };
    static SESSION_LAYOUT_FILE_FAULT: std::cell::RefCell<Option<SessionLayoutHook>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_session_layout_file_race(hook: impl FnOnce(&Path) + 'static) {
    SESSION_LAYOUT_FILE_RACE.with(|slot| slot.replace(Some(Box::new(hook))));
}

#[cfg(test)]
pub(crate) fn set_session_layout_dir_fault(hook: impl FnOnce(&Path) + 'static) {
    SESSION_LAYOUT_DIR_FAULT.with(|slot| slot.replace(Some(Box::new(hook))));
}

#[cfg(test)]
pub(crate) fn set_session_layout_file_fault(hook: impl FnOnce(&Path) + 'static) {
    SESSION_LAYOUT_FILE_FAULT.with(|slot| slot.replace(Some(Box::new(hook))));
}

fn record_text(
    path: &Path,
    content: &str,
    receipts: &mut SessionLayoutReceipts,
) -> Result<(), DurableSessionLayoutError> {
    write_text_file_if_missing(path, content, receipts)
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

pub(crate) fn create_dir(
    path: &Path,
    receipts: &mut SessionLayoutReceipts,
) -> Result<(), DurableSessionLayoutError> {
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
    let mut parent_dir =
        open_plain_directory(parent).map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    for dir in missing.iter().rev() {
        let name =
            plain_file_name(dir).map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
        let created = match nix::sys::stat::mkdirat(
            &parent_dir,
            name,
            nix::sys::stat::Mode::from_bits_truncate(0o700),
        ) {
            Ok(()) => true,
            Err(nix::errno::Errno::EEXIST) => false,
            Err(_error) => return Err(DurableSessionLayoutError::CannotCreate),
        };
        let child = nix::fcntl::openat(
            &parent_dir,
            name,
            nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map_err(|_error| {
            if created {
                DurableSessionLayoutError::RetainedResidue
            } else {
                DurableSessionLayoutError::CannotCreate
            }
        })?;
        let child = fs::File::from(child);
        let metadata = child.metadata().map_err(|_error| {
            if created {
                DurableSessionLayoutError::RetainedResidue
            } else {
                DurableSessionLayoutError::CannotCreate
            }
        })?;
        let rebound =
            nix::sys::stat::fstatat(&parent_dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                .map_err(|_error| {
                    if created {
                        DurableSessionLayoutError::RetainedResidue
                    } else {
                        DurableSessionLayoutError::CannotCreate
                    }
                })?;
        if !metadata.is_dir()
            || (metadata.dev(), metadata.ino()) != (rebound.st_dev, rebound.st_ino)
        {
            return Err(if created {
                DurableSessionLayoutError::RetainedResidue
            } else {
                DurableSessionLayoutError::CannotCreate
            });
        }
        if created {
            receipts.entries.push(SessionLayoutReceipt {
                path: dir.clone(),
                dev: metadata.dev(),
                ino: metadata.ino(),
                directory: true,
            });
            #[cfg(test)]
            if let Some(hook) = SESSION_LAYOUT_DIR_FAULT.with(|slot| slot.borrow_mut().take()) {
                hook(dir);
                return Err(DurableSessionLayoutError::CannotCreate);
            }
        }
        child
            .set_permissions(fs::Permissions::from_mode(0o700))
            .and_then(|()| child.sync_all())
            .and_then(|()| parent_dir.sync_all())
            .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
        parent_dir = child;
    }
    Ok(())
}

pub(crate) fn write_text_file_if_missing(
    path: &Path,
    content: &str,
    receipts: &mut SessionLayoutReceipts,
) -> Result<(), DurableSessionLayoutError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        return if metadata.is_file() {
            set_text_file_permissions(path)
        } else {
            Err(DurableSessionLayoutError::CannotCreate)
        };
    }
    if let Some(parent) = path.parent() {
        create_dir(parent, receipts)?;
    }
    write_private_text_file(path, content, receipts)
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
    let dir =
        open_plain_directory(path).map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    dir.set_permissions(fs::Permissions::from_mode(0o700))
        .and_then(|()| dir.sync_all())
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)
}

pub(crate) fn write_private_text_file(
    path: &Path,
    content: &str,
    receipts: &mut SessionLayoutReceipts,
) -> Result<(), DurableSessionLayoutError> {
    #[cfg(test)]
    if let Some(hook) = SESSION_LAYOUT_FILE_RACE.with(|slot| slot.borrow_mut().take()) {
        hook(path);
    }
    let (parent_dir, mut file) = match open_session_layout_file_at(
        path,
        nix::fcntl::OFlag::O_CREAT
            | nix::fcntl::OFlag::O_EXCL
            | nix::fcntl::OFlag::O_WRONLY
            | nix::fcntl::OFlag::O_NOFOLLOW
            | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    ) {
        Ok(files) => files,
        Err(DurableSessionLayoutError::CannotCreate)
            if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file()) =>
        {
            set_text_file_permissions(path)?;
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let metadata = file
        .metadata()
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    if !metadata.is_file() {
        return Err(DurableSessionLayoutError::CannotCreate);
    }
    let file_name =
        plain_file_name(path).map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    let rebound = nix::sys::stat::fstatat(
        &parent_dir,
        file_name,
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| DurableSessionLayoutError::RetainedResidue)?;
    if (metadata.dev(), metadata.ino()) != (rebound.st_dev, rebound.st_ino) {
        return Err(DurableSessionLayoutError::RetainedResidue);
    }
    receipts.entries.push(SessionLayoutReceipt {
        path: path.to_owned(),
        dev: metadata.dev(),
        ino: metadata.ino(),
        directory: false,
    });
    #[cfg(test)]
    if let Some(hook) = SESSION_LAYOUT_FILE_FAULT.with(|slot| slot.borrow_mut().take()) {
        hook(path);
        return Err(DurableSessionLayoutError::CannotCreate);
    }
    file.write_all(content.as_bytes())
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    file.set_permissions(fs::Permissions::from_mode(0o600))
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all())
        .and_then(|()| parent_dir.sync_all())
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    Ok(())
}

pub(crate) fn open_session_layout_file_at(
    path: &Path,
    flags: nix::fcntl::OFlag,
    mode: nix::sys::stat::Mode,
) -> Result<(fs::File, fs::File), DurableSessionLayoutError> {
    let parent = path
        .parent()
        .ok_or(DurableSessionLayoutError::CannotCreate)?;
    let file_name =
        plain_file_name(path).map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    let parent_dir =
        open_plain_directory(parent).map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
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
