#![expect(
    clippy::redundant_pub_crate,
    reason = "coordinator is shared across private agent, runtime, and tool modules"
)]

use cortexfs_tool_sdk::{Tool, ToolEmitter, ToolError, ToolInvocation, ToolResult, ToolSpec};
use serde_json::Map;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::Shutdown;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt as _, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

pub(crate) const AGENT_CREATE_SCHEMA: &str = r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "agent.create input",
  "description": "Create one parent-owned attenuated child agent.",
  "type": "object",
  "additionalProperties": false,
  "required": ["name", "handoff"],
  "properties": {
    "name": { "type": "string" },
    "handoff": { "type": "string" },
    "life": { "enum": ["owned", "temp"] },
    "path": { "type": "string" },
    "window": { "type": "integer", "minimum": 1, "maximum": 4294967295 }
  }
}"#;
const REFERENCE_OBJECT_RUNNER: &str = cortexfs_paths::CORTEXFS_OBJECT_RUNNER;
#[cfg(test)]
static FORCE_PRODUCTION_CLAIM_CONFLICT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(test)]
thread_local! {
    static CAPTURE_AUTHORIZED_CHILD_TOOL_PATH: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
    static CAPTURE_AUTHORIZED_CHILD_TOOL_PATH_ENABLED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
    static CAPTURE_MATERIALIZED_CHILD_WINDOW: std::cell::RefCell<Option<String>> =
        const { std::cell::RefCell::new(None) };
    static CAPTURE_MATERIALIZED_CHILD_WINDOW_ENABLED: std::cell::Cell<bool> =
        const { std::cell::Cell::new(false) };
}

#[derive(Debug)]
pub(crate) struct AgentCreateTool;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AgentChildCreateError {
    errno: &'static str,
    message: String,
}

impl AgentChildCreateError {
    const fn new(errno: &'static str, message: String) -> Self {
        Self { errno, message }
    }

    fn message(&self) -> &str {
        &self.message
    }

    pub(crate) const fn errno(&self) -> &'static str {
        self.errno
    }
}

impl std::fmt::Display for AgentChildCreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for AgentChildCreateError {}

type ChildCreateResult<T> = Result<T, AgentChildCreateError>;

trait ChildCreateOps {
    type Agent;
    type Handoff;
    type Launch;

    fn create_agent(&mut self) -> ChildCreateResult<Self::Agent>;
    fn publish_handoff(&mut self, agent: &Self::Agent) -> ChildCreateResult<Self::Handoff>;
    fn launch(
        &mut self,
        agent: &Self::Agent,
        handoff: &Self::Handoff,
    ) -> ChildCreateResult<Self::Launch>;
    fn claim(
        &mut self,
        agent: &Self::Agent,
        handoff: &Self::Handoff,
        launch: &Self::Launch,
    ) -> ChildCreateResult<()>;
    fn dispatch(&mut self, launch: &Self::Launch) -> ChildCreateResult<()>;
    fn fail_dispatch(&mut self, handoff: &Self::Handoff) -> ChildCreateResult<()>;
    fn stop(&mut self, launch: &Self::Launch) -> ChildCreateResult<()>;
    fn rollback_handoff(&mut self, handoff: &Self::Handoff) -> ChildCreateResult<()>;
    fn rollback_agent(&mut self, agent: Self::Agent) -> ChildCreateResult<()>;
}

/// Executes the child creation sequence and rolls back on intermediate failures.
fn coordinate_child_phases<O: ChildCreateOps>(ops: &mut O) -> ChildCreateResult<O::Launch> {
    let agent = ops.create_agent()?;
    let handoff = match ops.publish_handoff(&agent) {
        Ok(receipt) => receipt,
        Err(error) => return Err(ops.rollback_agent(agent).err().unwrap_or(error)),
    };
    let launch = match ops.launch(&agent, &handoff) {
        Ok(receipt) => receipt,
        Err(error) => {
            let conflict = ops.rollback_handoff(&handoff).err();
            let agent_conflict = ops.rollback_agent(agent).err();
            return Err(conflict.or(agent_conflict).unwrap_or(error));
        }
    };
    if let Err(error) = ops.claim(&agent, &handoff, &launch) {
        let stop_conflict = ops.stop(&launch).err();
        let handoff_conflict = ops.rollback_handoff(&handoff).err();
        let agent_conflict = ops.rollback_agent(agent).err();
        return Err(stop_conflict
            .or(handoff_conflict)
            .or(agent_conflict)
            .unwrap_or(error));
    }
    if let Err(error) = ops.dispatch(&launch) {
        let stop_conflict = ops.stop(&launch).err();
        let terminal_conflict = ops.fail_dispatch(&handoff).err();
        let agent_conflict = ops.rollback_agent(agent).err();
        return Err(stop_conflict
            .or(terminal_conflict)
            .or(agent_conflict)
            .unwrap_or(error));
    }
    Ok(launch)
}

/// Checks authorization and, if allowed, runs the child phase coordination.
fn coordinate_authorized_child<O: ChildCreateOps>(
    authorized: bool,
    ops: &mut O,
) -> ChildCreateResult<O::Launch> {
    if !authorized {
        return Err(child_error("EACCES", "parent policy denies child creation"));
    }
    coordinate_child_phases(ops)
}

fn child_error(errno: &'static str, message: impl Into<String>) -> AgentChildCreateError {
    AgentChildCreateError::new(errno, message.into())
}

#[cfg(test)]
fn capture_authorized_child_tool_path(path: &str) -> bool {
    if !CAPTURE_AUTHORIZED_CHILD_TOOL_PATH_ENABLED.with(|enabled| enabled.replace(false)) {
        return false;
    }
    CAPTURE_AUTHORIZED_CHILD_TOOL_PATH.with(|capture| capture.replace(Some(path.to_owned())));
    true
}

impl Tool for AgentCreateTool {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "agent.create",
            description: "Create one parent-owned attenuated child agent.",
            input_schema: AGENT_CREATE_SCHEMA,
        }
    }

    fn call(
        &self,
        invocation: &ToolInvocation,
        output: &mut ToolEmitter<&mut dyn Write>,
    ) -> ToolResult<()> {
        let value = invocation.json()?;
        let object = value
            .as_object()
            .ok_or_else(|| ToolError::invalid("input must be a json object"))?;
        if object.keys().any(|key| {
            !matches!(
                key.as_str(),
                "name" | "handoff" | "life" | "path" | "window"
            )
        }) {
            return Err(ToolError::invalid("unknown agent.create field"));
        }
        let name = required_string(object, "name")?;
        let handoff = required_string(object, "handoff")?;
        let life = object.get("life").map_or(Ok("owned"), |value| {
            value
                .as_str()
                .ok_or_else(|| ToolError::invalid("life must be a string"))
        })?;
        crate::ChildLifecycle::parse_exact(life)
            .map_err(|_error| ToolError::invalid("life must be owned or temp"))?;
        let path = object
            .get("path")
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| ToolError::invalid("path must be a string"))
            })
            .transpose()?;
        let window = object
            .get("window")
            .map(|value| {
                value
                    .as_u64()
                    .and_then(|value| u32::try_from(value).ok())
                    .filter(|value| *value != 0)
                    .ok_or_else(|| ToolError::invalid("window must be a positive u32 integer"))
            })
            .transpose()?;
        let run = runtime_field("CTX_RUN_ID")
            .map_err(|error| ToolError::new(error.errno(), error.message()))?;
        let child_session = format!("{name}-{run}");
        let launched = crate::runtime::control::create_child_from_environment(
            crate::runtime::control::CreateChildEnvironmentRequest {
                request_id: invocation.run_id(),
                child: name,
                child_session: &child_session,
                path,
                window,
                input: handoff,
                life,
            },
        )
        .map_err(|error| ToolError::new(error.errno(), error.to_string()))?;
        output
            .message(&format!(
                "child {name} active session={} pid={}",
                launched.child_session, launched.pid
            ))
            .map_err(|error| ToolError::new("EIO", error.to_string()))
    }
}

/// Reads a required string field from a JSON object or returns an error.
fn required_string<'a>(
    object: &'a Map<String, serde_json::Value>,
    field: &str,
) -> ToolResult<&'a str> {
    object
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ToolError::invalid(format!("{field} must be a string")))
}

#[expect(
    clippy::too_many_lines,
    reason = "compensated create transaction is kept in strict phase order"
)]
#[cfg(test)]
pub(crate) fn create_child(
    name: &str,
    handoff: &str,
    life: &str,
) -> ChildCreateResult<(String, u32)> {
    let parent = runtime_field("CTX_AGENT")?;
    let session = runtime_field("CTX_SESSION")?;
    let run = runtime_field("CTX_RUN_ID")?;
    let source = PathBuf::from(runtime_field("CTX_SOURCE")?);
    let root = PathBuf::from(runtime_field("CTX_ROOT")?);
    create_child_context(
        &source, &root, &parent, &session, &run, name, None, None, None, handoff, life,
    )
}

#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "compensated create transaction is kept in strict phase order"
)]
#[expect(
    clippy::redundant_pub_crate,
    reason = "runtime socket execution consumes this transaction across module boundaries"
)]
pub(crate) fn create_child_context(
    source: &Path,
    root: &Path,
    parent: &str,
    session: &str,
    run: &str,
    name: &str,
    requested_child_session: Option<&str>,
    requested_tool_path: Option<&str>,
    requested_window: Option<u32>,
    handoff: &str,
    life: &str,
) -> ChildCreateResult<(String, u32)> {
    if !crate::is_object_name(name)
        || !crate::is_object_name(parent)
        || !crate::is_object_name(session)
        || !crate::is_object_name(run)
        || !root.is_absolute()
        || !source.is_absolute()
        || handoff.contains('\0')
    {
        return Err(child_error(
            "EINVAL",
            "invalid agent.create runtime context",
        ));
    }
    let view = crate::derive_agent_runtime_view(source, parent)
        .map_err(|error| child_error(error.errno(), "cannot derive parent authority"))?;
    let child_window = crate::agent::window::attenuate_child_window(
        view.effective_window(),
        view.model_limit(),
        requested_window,
    )
    .map_err(|error| match error {
        crate::agent::window::ChildWindowError::Zero => {
            child_error("EINVAL", "child window must be positive")
        }
        crate::agent::window::ChildWindowError::UnknownParent => {
            child_error("EACCES", "parent effective window is unknown")
        }
        crate::agent::window::ChildWindowError::UnknownModel => {
            child_error("EACCES", "child model window is unknown")
        }
        crate::agent::window::ChildWindowError::ExceedsParent => {
            child_error("EACCES", "child window exceeds parent")
        }
        crate::agent::window::ChildWindowError::ExceedsModel => {
            child_error("EACCES", "child window exceeds model")
        }
    })?;
    let requested_tool_path = match requested_tool_path {
        Some(path)
            if path.is_empty()
                || path.split(':').any(str::is_empty)
                || crate::agent::view::validate_agent_ctx_path(path).is_err() =>
        {
            return Err(child_error("EINVAL", "invalid child tool path"));
        }
        Some(path) => Some(crate::ToolPath::parse(path)),
        None => None,
    };
    require_child_lifecycle_authority(&view, name)?;
    let lifecycle = crate::ChildLifecycle::parse_exact(life)
        .map_err(|_error| child_error("EINVAL", "life must be owned or temp"))?;
    let child_subject = view.policy_subject();
    let parent_ref = format!("agent:{parent} session:{session} run:{run}");
    let child_session =
        requested_child_session.map_or_else(|| format!("{name}-{run}"), str::to_owned);
    if !crate::is_object_name(&child_session) {
        return Err(child_error("EINVAL", "invalid child session"));
    }
    let policy_text = read_control(source, parent, "policy")?;
    let mount_text = read_control(source, parent, "mount")?;
    let child_policy = crate::PolicyV0::parse(&policy_text)
        .map_err(|_error| child_error("EINVAL", "invalid derived child policy"))?;
    let child_mounts = crate::MountTable::parse(&mount_text)
        .map_err(|_error| child_error("EINVAL", "invalid derived child mounts"))?;
    let child_tool_path = crate::authorize_child_agent(
        crate::ChildAgentRequest::new(
            name,
            &parent_ref,
            lifecycle,
            crate::ChildAgentControls::new(
                view.identity(),
                child_subject,
                &child_policy,
                &child_mounts,
                requested_tool_path.as_ref(),
            ),
        ),
        crate::ChildAgentAuthority::new(
            parent,
            view.identity(),
            view.policy_subject(),
            view.policy(),
            view.mount_table(),
            view.tool_path(),
        ),
    )
    .map_err(|_error| child_error("EACCES", "child authority exceeds parent"))?;
    let path = child_tool_path
        .dirs()
        .iter()
        .map(|dir| {
            dir.to_str()
                .map(str::to_owned)
                .ok_or_else(|| child_error("EINVAL", "invalid child tool path"))
        })
        .collect::<ChildCreateResult<Vec<_>>>()?
        .join(":");
    #[cfg(test)]
    if capture_authorized_child_tool_path(&path) {
        return Err(child_error("EAGAIN", "captured authorized child tool path"));
    }

    let uid = view.identity().uid().to_string();
    let groups = view
        .identity()
        .groups()
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let label = view.label().to_owned();
    let executable = child_executable(name);
    let cwd = view.cwd().to_str().unwrap_or("/workspace");
    let overrides = vec![
        ("owner".to_owned(), uid.clone()),
        ("uid".to_owned(), uid.clone()),
        ("gid".to_owned(), view.identity().gid().to_string()),
        ("groups".to_owned(), groups),
        (
            "perm".to_owned(),
            view.permissions().control().trim_end().to_owned(),
        ),
        ("label".to_owned(), label),
        ("parent".to_owned(), parent_ref),
        ("life".to_owned(), life.to_owned()),
        (
            "root".to_owned(),
            cortexfs_paths::ctx_root().display().to_string(),
        ),
        ("cwd".to_owned(), cwd.to_owned()),
        (
            "env".to_owned(),
            format!("CTX_ROOT={}", cortexfs_paths::CTX_ROOT),
        ),
        ("path".to_owned(), path),
        ("mount".to_owned(), mount_text),
        ("model".to_owned(), view.model().to_owned()),
        ("window".to_owned(), child_window.value()),
        ("policy".to_owned(), policy_text),
        ("status".to_owned(), "idle".to_owned()),
    ];
    let child_session_root = cortexfs_paths::agent_sessions_path(source, &uid, name);
    let parent_session = cortexfs_paths::agent_session_path(source, &uid, parent, session);
    let model = view.model().to_owned();
    let mut ops = ProductionOps {
        source,
        uid: &uid,
        owner_uid: view.identity().uid(),
        owner_gid: view.identity().gid(),
        name,
        executable: &executable,
        overrides,
        child_session_root,
        child_session: &child_session,
        cwd,
        model: &model,
        parent_session,
        handoff,
    };
    let launch = coordinate_authorized_child(true, &mut ops)?;
    Ok((child_session, launch.pid))
}

struct ChildLaunch {
    terminal: crate::agent::launch::AgentLaunchReceipt,
    system: crate::agent::launch::SystemAgentSocketReceipt,
    pid: u32,
    session: String,
}

struct ProductionOps<'a> {
    source: &'a Path,
    uid: &'a str,
    owner_uid: u32,
    owner_gid: u32,
    name: &'a str,
    executable: &'a str,
    overrides: Vec<(String, String)>,
    child_session_root: PathBuf,
    child_session: &'a str,
    cwd: &'a str,
    model: &'a str,
    parent_session: PathBuf,
    handoff: &'a str,
}

impl ChildCreateOps for ProductionOps<'_> {
    type Agent = crate::agent::create::AgentCreatePaths;
    type Handoff = crate::ChildHandoffReceipt;
    type Launch = ChildLaunch;

    fn create_agent(&mut self) -> ChildCreateResult<Self::Agent> {
        let overrides = self
            .overrides
            .iter()
            .map(|entry| (entry.0.as_str(), entry.1.as_str()))
            .collect::<Vec<_>>();
        let mut paths = crate::agent::create::create_agent_files(
            self.source,
            self.uid,
            self.name,
            self.executable,
            &overrides,
        )
        .map_err(child_create_error)?;
        paths = finish_session_layout(
            paths,
            crate::ensure_durable_session_layout(
                &self.child_session_root,
                self.child_session,
                self.cwd,
                Some(self.model),
                crate::SocketSessionScope::Private,
            ),
        )?;
        if crate::runtime::record::session::repair_agent_session_permissions(
            &self.child_session_root,
            self.owner_uid,
            self.owner_gid,
        )
        .is_err()
        {
            return rollback_create(paths, child_error("EIO", "cannot secure child session"));
        }
        Ok(paths)
    }

    fn publish_handoff(&mut self, _agent: &Self::Agent) -> ChildCreateResult<Self::Handoff> {
        #[cfg(test)]
        if CAPTURE_MATERIALIZED_CHILD_WINDOW_ENABLED.with(std::cell::Cell::get) {
            // Prefer in-memory override over disk re-read (avoids CI races).
            let content = self
                .overrides
                .iter()
                .find(|pair| pair.0 == "window")
                .map(|pair| format!("{}\n", pair.1.trim_end_matches('\n')))
                .ok_or_else(|| child_error("EIO", "cannot capture child window"))?;
            CAPTURE_MATERIALIZED_CHILD_WINDOW.with(|capture| capture.replace(Some(content)));
            return Err(child_error("EAGAIN", "captured materialized child window"));
        }
        let receipt = crate::publish_child_handoff(
            &self.parent_session,
            self.name,
            self.name,
            self.child_session,
            self.handoff,
        )
        .map_err(|error| child_error(error.errno(), "cannot record child handoff"))?;
        if crate::runtime::record::session::repair_agent_session_permissions(
            &self.parent_session,
            self.owner_uid,
            self.owner_gid,
        )
        .is_err()
        {
            return match crate::rollback_child_handoff(&receipt) {
                Ok(()) => Err(child_error("EIO", "cannot secure child handoff")),
                Err(error) => Err(child_error(
                    error.errno(),
                    "child handoff rollback conflict",
                )),
            };
        }
        Ok(receipt)
    }

    fn launch(
        &mut self,
        _agent: &Self::Agent,
        _handoff: &Self::Handoff,
    ) -> ChildCreateResult<Self::Launch> {
        let view = crate::derive_agent_runtime_view(self.source, self.name)
            .map_err(|error| child_error(error.errno(), "cannot derive child runtime"))?;
        launch_child(self.source, &view, self.child_session)
    }

    fn claim(
        &mut self,
        _agent: &Self::Agent,
        handoff: &Self::Handoff,
        launch: &Self::Launch,
    ) -> ChildCreateResult<()> {
        crate::agent::launch::persist_agent_launch_meta(
            self.source,
            self.name,
            &launch.terminal,
            &launch.system,
        )
        .map_err(|_error| child_error("EIO", "cannot publish child runtime receipt"))?;
        #[cfg(test)]
        if FORCE_PRODUCTION_CLAIM_CONFLICT.swap(false, std::sync::atomic::Ordering::AcqRel) {
            return Err(child_error("EIO", "forced child claim conflict"));
        }
        crate::claim_child_handoff_active(
            handoff,
            self.name,
            self.child_session,
            Some(self.handoff),
        )
        .map_err(|error| child_error(error.errno(), "cannot activate child handoff"))
    }

    fn stop(&mut self, launch: &Self::Launch) -> ChildCreateResult<()> {
        stop_child(launch)
    }

    fn dispatch(&mut self, launch: &Self::Launch) -> ChildCreateResult<()> {
        dispatch_child_handoff(
            self.source,
            self.name,
            &launch.session,
            self.cwd,
            self.handoff,
        )
    }

    fn fail_dispatch(&mut self, handoff: &Self::Handoff) -> ChildCreateResult<()> {
        crate::finish_child_result(
            handoff,
            self.name,
            self.child_session,
            crate::ChildContextStatus::Error,
            "child handoff dispatch failed",
            "",
        )
        .map_err(|error| child_error(error.errno(), "cannot terminalize child dispatch failure"))
    }

    fn rollback_handoff(&mut self, handoff: &Self::Handoff) -> ChildCreateResult<()> {
        crate::rollback_child_handoff(handoff)
            .map_err(|_error| child_error("EIO", "child handoff rollback conflict"))
    }

    fn rollback_agent(&mut self, agent: Self::Agent) -> ChildCreateResult<()> {
        crate::agent::create::rollback_agent_files(agent).map_err(|error| match error {
            crate::agent::create::AgentRollbackError::Conflict(conflict) => child_error(
                "EIO",
                format!(
                    "agent create rollback conflict: {}",
                    crate::agent::create::format_agent_rollback_conflict(&conflict)
                ),
            ),
        })
    }
}

fn finish_session_layout(
    mut paths: crate::agent::create::AgentCreatePaths,
    result: Result<crate::SessionLayoutReceipts, crate::DurableSessionLayoutError>,
) -> ChildCreateResult<crate::agent::create::AgentCreatePaths> {
    match result {
        Ok(receipts) => {
            paths.own_session_layout(receipts);
            Ok(paths)
        }
        Err(error) => rollback_create(
            paths,
            child_error("EIO", format!("cannot prepare child session: {error:?}")),
        ),
    }
}

#[derive(Clone, Copy)]
struct StartupStubReceipt {
    dev: u64,
    ino: u64,
    created: bool,
}

fn prepare_startup_stub(parent: &fs::File, uid: u32, gid: u32) -> io::Result<StartupStubReceipt> {
    let flags =
        nix::fcntl::OFlag::O_RDWR | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC;
    let (fd, created) = match nix::fcntl::openat(
        parent,
        ".empty-shell-startup",
        flags | nix::fcntl::OFlag::O_CREAT | nix::fcntl::OFlag::O_EXCL,
        nix::sys::stat::Mode::from_bits_truncate(0o600),
    ) {
        Ok(fd) => (fd, true),
        Err(nix::errno::Errno::EEXIST) => (
            nix::fcntl::openat(
                parent,
                ".empty-shell-startup",
                flags,
                nix::sys::stat::Mode::empty(),
            )
            .map_err(io::Error::from)?,
            false,
        ),
        Err(error) => return Err(io::Error::from(error)),
    };
    let file = fs::File::from(fd);
    // Without an fd-derived inode receipt, unlinking by pathname could delete a replacement.
    let metadata = file.metadata().map_err(|error| {
        io::Error::other(format!(
            "startup stub cleanup conflict: cannot establish inode receipt: {error}"
        ))
    })?;
    let receipt = StartupStubReceipt {
        dev: metadata.dev(),
        ino: metadata.ino(),
        created,
    };
    let result = (|| {
        if created {
            nix::unistd::fchown(
                &file,
                Some(nix::unistd::Uid::from_raw(uid)),
                Some(nix::unistd::Gid::from_raw(gid)),
            )
            .map_err(io::Error::from)?;
            nix::sys::stat::fchmod(&file, nix::sys::stat::Mode::from_bits_truncate(0o600))
                .map_err(io::Error::from)?;
        }
        let metadata = file.metadata()?;
        if !metadata.is_file()
            || metadata.nlink() != 1
            || metadata.uid() != uid
            || metadata.gid() != gid
            || metadata.permissions().mode() & 0o7777 != 0o600
        {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        let rebound = nix::sys::stat::fstatat(
            parent,
            ".empty-shell-startup",
            nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
        )
        .map_err(io::Error::from)?;
        if (rebound.st_dev, rebound.st_ino) != (receipt.dev, receipt.ino) {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        file.set_len(0)?;
        file.sync_all()?;
        Ok(receipt)
    })();
    match result {
        Ok(receipt) => Ok(receipt),
        Err(error) if !created => Err(error),
        Err(error) => {
            cleanup_created_startup_stub(parent, receipt).map_or_else(Err, |()| Err(error))
        }
    }
}

fn cleanup_created_startup_stub(parent: &fs::File, receipt: StartupStubReceipt) -> io::Result<()> {
    if !receipt.created {
        return Ok(());
    }
    let matches = nix::sys::stat::fstatat(
        parent,
        ".empty-shell-startup",
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .is_ok_and(|stat| {
        (stat.st_dev, stat.st_ino) == (receipt.dev, receipt.ino) && stat.st_nlink == 1
    });
    if !matches {
        return Err(io::Error::other("startup stub cleanup conflict"));
    }
    nix::unistd::unlinkat(
        parent,
        ".empty-shell-startup",
        nix::unistd::UnlinkatFlags::NoRemoveDir,
    )
    .map_err(|error| io::Error::other(format!("startup stub cleanup conflict: {error}")))
}

fn startup_failure(
    parent: &fs::File,
    receipt: StartupStubReceipt,
    original: AgentChildCreateError,
) -> AgentChildCreateError {
    cleanup_created_startup_stub(parent, receipt)
        .map_or_else(|error| child_error("EIO", error.to_string()), |()| original)
}

fn launch_child(
    source: &Path,
    view: &crate::AgentRuntimeView,
    session: &str,
) -> ChildCreateResult<ChildLaunch> {
    use std::time::Duration;
    let runtime = cortexfs_paths::user_runtime_root(view.identity().uid());
    let terminal_socket =
        cortexfs_paths::terminal_runtime_socket(&runtime, view.agent_name(), session);
    let terminal_fd = crate::agent::launch::ensure_terminal_runtime_dir(
        &runtime,
        view.agent_name(),
        session,
        view.identity(),
    )
    .map_err(|error| child_error("EIO", error.to_string()))?;
    let startup = prepare_startup_stub(&terminal_fd, view.identity().uid(), view.identity().gid())
        .map_err(|error| child_error("EIO", error.to_string()))?;
    let terminal_unit = format!("cortexfs-agent-{}-{session}-terminal", view.agent_name());
    crate::agent::launch::reset_unit_for(view.identity(), &terminal_unit);
    let request = crate::agent::launch::AgentLaunchRequest {
        agent: view.agent_name().to_owned(),
        session: session.to_owned(),
        source: source.to_path_buf(),
        cwd: view.cwd().display().to_string(),
        mounts: Vec::new(),
        default_workspace: false,
    };
    let command =
        crate::agent::launch::terminal_command(&request, view, &terminal_socket, &terminal_unit);
    let output = match crate::agent::launch::launch_process_for(view.identity(), &command)
        .and_then(|mut command| command.output())
    {
        Ok(output) => output,
        Err(error) => {
            return Err(startup_failure(
                &terminal_fd,
                startup,
                child_error("EIO", error.to_string()),
            ));
        }
    };
    if !output.status.success()
        || crate::agent::launch::wait_socket(&terminal_socket, 50, Duration::from_millis(100))
            .is_err()
    {
        crate::agent::launch::reset_unit_for(view.identity(), &terminal_unit);
        return Err(startup_failure(
            &terminal_fd,
            startup,
            child_error("EIO", "child terminal did not become ready"),
        ));
    }
    let terminal =
        crate::agent::launch::launch_receipt(view.identity(), &terminal_unit, terminal_socket)
            .map_err(|_error| {
                crate::agent::launch::reset_unit_for(view.identity(), &terminal_unit);
                startup_failure(
                    &terminal_fd,
                    startup,
                    child_error("EIO", "child terminal has no live pid"),
                )
            })?;
    let pid = terminal.pid;
    let chat_visible = cortexfs_paths::agent_backing_socket(source, view.agent_name());
    let system =
        match crate::agent::launch::ensure_system_agent_socket(view.agent_name(), &chat_visible) {
            Ok(receipt) => receipt,
            Err(_error) => {
                let original = crate::agent::launch::stop_launch(&terminal)
                    .err()
                    .map_or_else(
                        || child_error("EIO", "child system chat socket did not become ready"),
                        |_conflict| child_error("EIO", "child terminal cleanup conflict"),
                    );
                return Err(startup_failure(&terminal_fd, startup, original));
            }
        };
    for (file, value) in [
        ("status", "ready\n".to_owned()),
        ("pid", format!("{pid}\n")),
    ] {
        if let Err(error) = write_control(
            &cortexfs_paths::agent_control_file_path(source, view.agent_name(), file),
            &value,
        ) {
            let launch = ChildLaunch {
                terminal,
                system,
                pid,
                session: session.to_owned(),
            };
            let original = stop_child(&launch)
                .err()
                .unwrap_or_else(|| child_error("EIO", error.to_string()));
            return Err(startup_failure(&terminal_fd, startup, original));
        }
    }
    Ok(ChildLaunch {
        terminal,
        system,
        pid,
        session: session.to_owned(),
    })
}

fn write_control(path: &Path, value: &str) -> io::Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "agent control is not a regular file",
        ));
    }
    file.write_all(value.as_bytes())?;
    file.sync_all()
}

fn dispatch_child_handoff(
    source: &Path,
    child: &str,
    session: &str,
    cwd: &str,
    handoff: &str,
) -> ChildCreateResult<()> {
    let socket = cortexfs_paths::agent_backing_socket(source, child);
    let run = format!("handoff-{session}");
    let request = serde_json::json!({
        "op": "send",
        "id": run,
        "session": session,
        "scope": "private",
        "cwd": cwd,
        "input": handoff,
    });
    let mut stream = UnixStream::connect(&socket)
        .map_err(|_error| child_error("EIO", "cannot dispatch child handoff"))?;
    stream
        .set_write_timeout(Some(std::time::Duration::from_secs(5)))
        .and_then(|()| stream.set_read_timeout(Some(std::time::Duration::from_secs(5))))
        .and_then(|()| stream.write_all(request.to_string().as_bytes()))
        .and_then(|()| stream.write_all(b"\n"))
        .and_then(|()| stream.flush())
        .and_then(|()| stream.shutdown(Shutdown::Write))
        .map_err(|_error| child_error("EIO", "cannot submit child handoff"))?;
    let mut response = String::new();
    BufReader::new(&stream)
        .read_line(&mut response)
        .map_err(|_error| child_error("EIO", "cannot confirm child handoff"))?;
    let accepted = serde_json::from_str::<serde_json::Value>(&response).is_ok_and(|value| {
        value.get("type").and_then(serde_json::Value::as_str) == Some("start")
            && value.get("client_id").and_then(serde_json::Value::as_str) == Some(run.as_str())
            && value
                .get("run")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|value| !value.is_empty())
    });
    if !accepted {
        return Err(child_error("EIO", "child handoff was not durably recorded"));
    }
    Ok(())
}

fn stop_child(launch: &ChildLaunch) -> ChildCreateResult<()> {
    stop_child_with(
        || crate::agent::launch::stop_launch(&launch.terminal),
        || crate::agent::launch::stop_system_agent_socket(&launch.system),
    )
}

fn stop_child_with(
    terminal: impl FnOnce() -> Result<(), crate::agent::launch::AgentLaunchError>,
    system: impl FnOnce() -> Result<(), crate::agent::launch::AgentLaunchError>,
) -> ChildCreateResult<()> {
    terminal().map_err(|_error| child_error("EIO", "child terminal cleanup conflict"))?;
    system().map_err(|_error| child_error("EIO", "child system socket cleanup conflict"))
}

fn rollback_create<T>(
    paths: crate::agent::create::AgentCreatePaths,
    error: AgentChildCreateError,
) -> ChildCreateResult<T> {
    match crate::agent::create::rollback_agent_files(paths) {
        Ok(()) => Err(error),
        Err(crate::agent::create::AgentRollbackError::Conflict(conflict)) => Err(child_error(
            "EIO",
            format!(
                "{error}; agent create rollback conflict: {}",
                crate::agent::create::format_agent_rollback_conflict(&conflict)
            ),
        )),
    }
}

pub(crate) fn child_executable(name: &str) -> String {
    crate::executable_wrapper_script(crate::ObjectClass::Agent, name, REFERENCE_OBJECT_RUNNER)
}

fn require_child_lifecycle_authority(
    view: &crate::AgentRuntimeView,
    name: &str,
) -> ChildCreateResult<()> {
    let policy: &dyn crate::PolicyEvaluator = view.policy();
    if !policy.evaluate(
        view.policy_subject(),
        crate::PolicyObjectClass::Tool,
        "agent.create",
        crate::PolicyPermission::Execute,
    ) {
        return Err(child_error(
            "EACCES",
            "parent policy denies agent.create execution",
        ));
    }
    for (permission, message) in [
        (
            crate::PolicyPermission::Create,
            "parent policy denies child creation",
        ),
        (
            crate::PolicyPermission::Start,
            "parent policy denies child start",
        ),
    ] {
        if !policy.evaluate(
            view.policy_subject(),
            crate::PolicyObjectClass::Agent,
            name,
            permission,
        ) {
            return Err(child_error("EACCES", message));
        }
    }
    Ok(())
}

fn runtime_field(name: &str) -> ChildCreateResult<String> {
    std::env::var(name).map_err(|_error| child_error("EINVAL", format!("missing {name}")))
}

fn read_control(root: &Path, agent: &str, file: &str) -> ChildCreateResult<String> {
    fs::read_to_string(cortexfs_paths::agent_control_file_path(root, agent, file))
        .map_err(|_error| child_error("EIO", format!("cannot read parent {file}")))
}

#[expect(
    dead_code,
    reason = "withheld tool regression tests consume this SDK adapter"
)]
pub(crate) fn create_error(error: crate::agent::create::AgentCreateError) -> ToolError {
    let error = child_create_error(error);
    ToolError::new(error.errno(), error.message())
}

fn child_create_error(error: crate::agent::create::AgentCreateError) -> AgentChildCreateError {
    match error {
        crate::agent::create::AgentCreateError::InvalidInput => {
            child_error("EINVAL", "invalid child controls")
        }
        crate::agent::create::AgentCreateError::AlreadyExists => {
            child_error("EEXIST", "child already exists")
        }
        crate::agent::create::AgentCreateError::CannotCreate => {
            child_error("EIO", "cannot create child")
        }
        crate::agent::create::AgentCreateError::RollbackConflict(conflict) => child_error(
            "EIO",
            format!(
                "agent create rollback conflict: {}",
                crate::agent::create::format_agent_rollback_conflict(&conflict)
            ),
        ),
    }
}

#[cfg(test)]
mod d_tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::net::UnixListener;

    #[derive(Clone, Debug)]
    struct FixtureNode {
        path: PathBuf,
        dev: u64,
        ino: u64,
        mode: u32,
        uid: u32,
        gid: u32,
    }

    fn record_fixture_tree(root: &Path) -> io::Result<Vec<FixtureNode>> {
        let root_meta = fs::symlink_metadata(root)?;
        if !root_meta.file_type().is_dir() {
            return Err(io::Error::other("fixture root is not a directory"));
        }
        let root_dev = root_meta.dev();
        let mut pending = vec![PathBuf::new()];
        let mut nodes = Vec::new();
        while let Some(relative) = pending.pop() {
            let directory = root.join(&relative);
            for entry in fs::read_dir(&directory)? {
                let entry = entry?;
                let name = entry.file_name();
                if name.to_str().is_none() || name == "." || name == ".." {
                    return Err(io::Error::other("invalid fixture entry"));
                }
                let path = relative.join(name);
                let metadata = fs::symlink_metadata(root.join(&path))?;
                let file_type = metadata.file_type();
                let kind = nix::sys::stat::SFlag::from_bits_truncate(metadata.mode());
                if metadata.dev() != root_dev
                    || !(kind == nix::sys::stat::SFlag::S_IFDIR
                        || kind == nix::sys::stat::SFlag::S_IFREG
                        || kind == nix::sys::stat::SFlag::S_IFLNK
                        || kind == nix::sys::stat::SFlag::S_IFSOCK)
                {
                    return Err(io::Error::other("special or mounted fixture entry"));
                }
                nodes.push(FixtureNode {
                    path: path.clone(),
                    dev: metadata.dev(),
                    ino: metadata.ino(),
                    mode: metadata.mode(),
                    uid: metadata.uid(),
                    gid: metadata.gid(),
                });
                if file_type.is_dir() {
                    pending.push(path);
                }
            }
        }
        Ok(nodes)
    }

    fn cleanup_fixture_tree(root: &Path, mut nodes: Vec<FixtureNode>) -> io::Result<()> {
        nodes.sort_by_key(|node| std::cmp::Reverse(node.path.components().count()));
        for node in nodes {
            let parent = node
                .path
                .parent()
                .map_or_else(|| root.to_path_buf(), |path| root.join(path));
            let name = node
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| io::Error::other("invalid fixture entry"))?;
            let parent = crate::support::plain::open_plain_directory(&parent)?;
            let stat =
                nix::sys::stat::fstatat(&parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
                    .map_err(io::Error::from)?;
            if stat.st_dev != node.dev
                || stat.st_ino != node.ino
                || stat.st_mode != node.mode
                || stat.st_uid != node.uid
                || stat.st_gid != node.gid
            {
                return Err(io::Error::other("fixture entry replacement conflict"));
            }
            let flags = if nix::sys::stat::SFlag::from_bits_truncate(node.mode)
                .contains(nix::sys::stat::SFlag::S_IFDIR)
            {
                nix::unistd::UnlinkatFlags::RemoveDir
            } else {
                nix::unistd::UnlinkatFlags::NoRemoveDir
            };
            nix::unistd::unlinkat(&parent, name, flags).map_err(io::Error::from)?;
        }
        Ok(())
    }

    fn fixture_additions(
        baseline: &[FixtureNode],
        current: Vec<FixtureNode>,
    ) -> io::Result<Vec<FixtureNode>> {
        current
            .into_iter()
            .filter_map(|node| {
                let Some(baseline) = baseline.iter().find(|baseline| baseline.path == node.path)
                else {
                    return Some(Ok(node));
                };
                let baseline_kind = nix::sys::stat::SFlag::from_bits_truncate(baseline.mode);
                let node_kind = nix::sys::stat::SFlag::from_bits_truncate(node.mode);
                if (
                    baseline.dev,
                    baseline.ino,
                    baseline_kind,
                    baseline.uid,
                    baseline.gid,
                ) == (node.dev, node.ino, node_kind, node.uid, node.gid)
                {
                    None
                } else {
                    Some(Err(io::Error::other(
                        "baseline fixture entry replacement conflict",
                    )))
                }
            })
            .collect()
    }

    #[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
    enum FaultStage {
        Ready,
        AfterAgent,
        AfterHandoff,
        AfterLaunch,
        BeforeDispatch,
        #[default]
        BeforeClaim,
    }

    #[derive(Default)]
    struct TestOps {
        fault: FaultStage,
        conflict: Option<&'static str>,
        resources: BTreeSet<&'static str>,
        compensation: Vec<&'static str>,
        phases: Vec<&'static str>,
    }

    impl ChildCreateOps for TestOps {
        type Agent = ();
        type Handoff = ();
        type Launch = ();

        fn create_agent(&mut self) -> ChildCreateResult<Self::Agent> {
            self.resources.insert("agent");
            if self.fault == FaultStage::AfterAgent {
                self.resources.remove("agent");
                return Err(child_error("EIO", "create"));
            }
            Ok(())
        }

        fn publish_handoff(&mut self, _agent: &Self::Agent) -> ChildCreateResult<Self::Handoff> {
            self.resources.insert("channel");
            if self.fault == FaultStage::AfterHandoff {
                self.resources.remove("channel");
                return Err(child_error("EIO", "handoff"));
            }
            Ok(())
        }

        fn launch(
            &mut self,
            _agent: &Self::Agent,
            _handoff: &Self::Handoff,
        ) -> ChildCreateResult<Self::Launch> {
            self.resources.extend(["unit", "socket"]);
            if self.fault == FaultStage::AfterLaunch {
                self.resources.remove("unit");
                self.resources.remove("socket");
                return Err(child_error("EIO", "launch"));
            }
            Ok(())
        }

        fn claim(
            &mut self,
            _agent: &Self::Agent,
            _handoff: &Self::Handoff,
            _launch: &Self::Launch,
        ) -> ChildCreateResult<()> {
            self.phases.push("claim");
            if self.fault == FaultStage::BeforeClaim {
                Err(child_error("EIO", "claim"))
            } else {
                Ok(())
            }
        }

        fn stop(&mut self, _launch: &Self::Launch) -> ChildCreateResult<()> {
            self.compensation.push("stop");
            self.resources.remove("unit");
            self.resources.remove("socket");
            self.resources.remove("request");
            if self.conflict == Some("stop") {
                Err(child_error("EIO", "stop conflict"))
            } else {
                Ok(())
            }
        }

        fn dispatch(&mut self, _launch: &Self::Launch) -> ChildCreateResult<()> {
            self.phases.push("dispatch");
            if self.fault == FaultStage::BeforeDispatch {
                Err(child_error("EIO", "dispatch"))
            } else {
                self.resources.insert("request");
                Ok(())
            }
        }

        fn fail_dispatch(&mut self, _handoff: &Self::Handoff) -> ChildCreateResult<()> {
            self.compensation.push("terminal");
            self.resources.remove("request");
            Ok(())
        }

        fn rollback_handoff(&mut self, _handoff: &Self::Handoff) -> ChildCreateResult<()> {
            self.compensation.push("handoff");
            self.resources.remove("channel");
            if self.conflict == Some("handoff") {
                Err(child_error("EIO", "handoff conflict"))
            } else {
                Ok(())
            }
        }

        fn rollback_agent(&mut self, _agent: Self::Agent) -> ChildCreateResult<()> {
            self.compensation.push("agent");
            self.resources.remove("agent");
            if self.conflict == Some("agent") {
                Err(child_error("EIO", "agent conflict complete receipt"))
            } else {
                Ok(())
            }
        }
    }

    fn capture_production_child_tool_path(
        parent_path: &str,
        requested_path: Option<&str>,
    ) -> (Option<(String, String)>, Option<String>) {
        let source = tempfile::tempdir();
        assert!(source.is_ok(), "tempdir: {source:?}");
        let Ok(source) = source else {
            return (None, None);
        };
        let ensured = crate::ensure_reference_tree(source.path());
        assert!(ensured.is_ok(), "{ensured:?}");
        let control = source.path().join("agent/coder.d");
        assert!(fs::write(control.join("path"), format!("{parent_path}\n")).is_ok());
        assert!(fs::write(control.join("model"), "debug/echo\n").is_ok());
        assert!(fs::write(control.join("window"), "auto\n").is_ok());
        let model_control = source.path().join("model/debug/echo.d");
        assert!(fs::create_dir_all(&model_control).is_ok());
        assert!(fs::write(model_control.join("limit"), "unknown\n").is_ok());
        assert!(
            fs::write(
                control.join("policy"),
                "allow coder_t tool:agent.create execute\n\
allow coder_t agent:worker create\n\
allow coder_t agent:worker start\n",
            )
            .is_ok()
        );
        CAPTURE_AUTHORIZED_CHILD_TOOL_PATH.with(|capture| {
            capture.take();
        });
        CAPTURE_AUTHORIZED_CHILD_TOOL_PATH_ENABLED.with(|enabled| enabled.set(true));
        let result = create_child_context(
            source.path(),
            Path::new("/ctx"),
            "coder",
            "default",
            "path-run",
            "worker",
            Some("worker-path-run"),
            requested_path,
            None,
            "path test",
            "owned",
        );
        CAPTURE_AUTHORIZED_CHILD_TOOL_PATH_ENABLED.with(|enabled| enabled.set(false));
        let captured = CAPTURE_AUTHORIZED_CHILD_TOOL_PATH.with(std::cell::RefCell::take);
        let error = result
            .err()
            .map(|error| (error.errno().to_owned(), error.message().to_owned()));
        (error, captured)
    }

    fn capture_production_child_window(
        parent_window: &str,
        model_limit: &str,
        requested: Option<u32>,
    ) -> (Option<(String, String)>, Option<String>) {
        let source = tempfile::tempdir();
        assert!(source.is_ok(), "tempdir: {source:?}");
        let Ok(source) = source else {
            return (None, None);
        };
        assert!(crate::ensure_reference_tree(source.path()).is_ok());
        let control = source.path().join("agent/coder.d");
        assert!(fs::write(control.join("model"), "debug/echo\n").is_ok());
        assert!(fs::write(control.join("window"), parent_window).is_ok());
        assert!(
            fs::write(
                control.join("policy"),
                "allow coder_t tool:agent.create execute\n\
allow coder_t agent:window-child create\n\
allow coder_t agent:window-child start\n",
            )
            .is_ok()
        );
        let model_control = source.path().join("model/debug/echo.d");
        assert!(fs::create_dir_all(&model_control).is_ok());
        assert!(fs::write(model_control.join("limit"), model_limit).is_ok());
        CAPTURE_MATERIALIZED_CHILD_WINDOW.with(|capture| {
            capture.take();
        });
        CAPTURE_MATERIALIZED_CHILD_WINDOW_ENABLED.with(|enabled| enabled.set(true));
        let result = create_child_context(
            source.path(),
            Path::new("/ctx"),
            "coder",
            "default",
            "window-run",
            "window-child",
            Some("window-child-run"),
            None,
            requested,
            "window test",
            "owned",
        );
        CAPTURE_MATERIALIZED_CHILD_WINDOW_ENABLED.with(|enabled| enabled.set(false));
        let captured = CAPTURE_MATERIALIZED_CHILD_WINDOW.with(std::cell::RefCell::take);
        assert!(!source.path().join("agent/window-child").exists());
        assert!(!source.path().join("agent/window-child.d").exists());
        let error = result
            .err()
            .map(|error| (error.errno().to_owned(), error.message().to_owned()));
        (error, captured)
    }

    #[test]
    fn production_child_without_request_materializes_known_parent_window() {
        let (error, window) = capture_production_child_window("auto\n", "64\n", None);
        assert_eq!(error.as_ref().map(|error| error.0.as_str()), Some("EAGAIN"));
        assert_eq!(window.as_deref(), Some("64\n"));
    }

    #[test]
    fn production_child_without_request_materializes_auto_for_unknown_parent() {
        let (error, window) = capture_production_child_window("auto\n", "unknown\n", None);
        assert_eq!(error.as_ref().map(|error| error.0.as_str()), Some("EAGAIN"));
        assert_eq!(window.as_deref(), Some("auto\n"));
    }

    #[test]
    fn production_child_materializes_equal_explicit_window_canonically() {
        let (error, window) = capture_production_child_window("auto\n", "64\n", Some(64));
        assert_eq!(error.as_ref().map(|error| error.0.as_str()), Some("EAGAIN"));
        assert_eq!(window.as_deref(), Some("64\n"));
    }

    #[test]
    fn production_child_materializes_smaller_explicit_window_canonically() {
        let (error, window) = capture_production_child_window("auto\n", "64\n", Some(32));
        assert_eq!(error.as_ref().map(|error| error.0.as_str()), Some("EAGAIN"));
        assert_eq!(window.as_deref(), Some("32\n"));
    }

    #[test]
    fn production_child_tool_path_inherits_duplicate_parent_exactly() {
        let (error, captured) =
            capture_production_child_tool_path("/ctx/home/1000/tool:/ctx/home/1000/tool", None);
        assert_eq!(
            error,
            Some((
                "EAGAIN".to_owned(),
                "captured authorized child tool path".to_owned()
            ))
        );
        assert_eq!(
            captured.as_deref(),
            Some("/ctx/home/1000/tool:/ctx/home/1000/tool")
        );
    }

    #[test]
    fn invalid_child_window_is_rejected_before_materialization() {
        let source = tempfile::tempdir();
        assert!(source.is_ok(), "tempdir: {source:?}");
        let Ok(source) = source else { return };
        assert!(crate::ensure_reference_tree(source.path()).is_ok());
        assert!(fs::write(source.path().join("agent/coder.d/model"), "debug/echo\n").is_ok());
        let model_control = source.path().join("model/debug/echo.d");
        assert!(fs::create_dir_all(&model_control).is_ok());
        assert!(fs::write(model_control.join("limit"), "unknown\n").is_ok());
        let parent_view = crate::derive_agent_runtime_view(source.path(), "coder");
        assert!(parent_view.is_ok(), "{parent_view:?}");
        let agent = source.path().join("agent/window-child.d");
        let home = source.path().join("home/1000/agent/window-child");

        let zero = create_child_context(
            source.path(),
            Path::new("/ctx"),
            "coder",
            "default",
            "window-run",
            "window-child",
            Some("window-child-session"),
            None,
            Some(0),
            "handoff",
            "owned",
        );
        assert!(matches!(zero, Err(ref error) if error.errno() == "EINVAL"));
        assert!(!agent.exists());
        assert!(!home.exists());

        let unknown = create_child_context(
            source.path(),
            Path::new("/ctx"),
            "coder",
            "default",
            "window-run",
            "window-child",
            Some("window-child-session"),
            None,
            Some(1),
            "handoff",
            "owned",
        );
        assert!(
            matches!(unknown, Err(ref error) if error.errno() == "EACCES"),
            "{unknown:?}"
        );
        assert!(!agent.exists());
        assert!(!home.exists());
    }

    #[test]
    fn production_child_tool_path_accepts_ordered_tier_deletion() {
        let (error, captured) = capture_production_child_tool_path(
            "/ctx/tool:/ctx/home/1000/tool:/ctx/shared/team/tool",
            Some("/ctx/home/1000/tool:/ctx/shared/team/tool"),
        );
        assert_eq!(error.as_ref().map(|error| error.0.as_str()), Some("EAGAIN"));
        assert_eq!(
            captured.as_deref(),
            Some("/ctx/home/1000/tool:/ctx/shared/team/tool")
        );
    }

    #[test]
    fn production_child_tool_path_rejects_explicit_empty_path() {
        let (error, captured) = capture_production_child_tool_path("/ctx/home/1000/tool", Some(""));
        assert_eq!(error.as_ref().map(|error| error.0.as_str()), Some("EINVAL"));
        assert_eq!(captured, None);
    }

    #[test]
    fn production_child_tool_path_rejects_duplicate_tier() {
        let (error, captured) = capture_production_child_tool_path(
            "/ctx/tool:/ctx/home/1000/tool",
            Some("/ctx/tool:/ctx/tool"),
        );
        assert_eq!(error.as_ref().map(|error| error.0.as_str()), Some("EACCES"));
        assert_eq!(captured, None);
    }

    #[test]
    fn production_child_tool_path_rejects_added_tier() {
        let (error, captured) = capture_production_child_tool_path(
            "/ctx/home/1000/tool",
            Some("/ctx/home/1000/tool:/ctx/shared/team/tool"),
        );
        assert_eq!(error.as_ref().map(|error| error.0.as_str()), Some("EACCES"));
        assert_eq!(captured, None);
    }

    #[test]
    fn production_child_tool_path_rejects_reordered_tiers() {
        let (error, captured) = capture_production_child_tool_path(
            "/ctx/tool:/ctx/home/1000/tool",
            Some("/ctx/home/1000/tool:/ctx/tool"),
        );
        assert_eq!(error.as_ref().map(|error| error.0.as_str()), Some("EACCES"));
        assert_eq!(captured, None);
    }

    #[test]
    fn production_child_tool_path_rejects_ctx_tool_escalation() {
        let (error, captured) =
            capture_production_child_tool_path("/ctx/home/1000/tool", Some("/ctx/tool"));
        assert_eq!(error.as_ref().map(|error| error.0.as_str()), Some("EACCES"));
        assert_eq!(captured, None);
    }

    #[test]
    fn d_fault_matrix_has_exact_compensation_order_and_no_residue() {
        for (stage, expected) in [
            (FaultStage::AfterAgent, &[][..]),
            (FaultStage::AfterHandoff, &["agent"][..]),
            (FaultStage::AfterLaunch, &["handoff", "agent"][..]),
        ] {
            let mut ops = TestOps {
                fault: stage,
                ..TestOps::default()
            };
            assert!(coordinate_child_phases(&mut ops).is_err());
            assert_eq!(ops.compensation, expected);
            assert!(ops.resources.is_empty());
        }

        let mut claim = TestOps {
            fault: FaultStage::BeforeClaim,
            ..TestOps::default()
        };
        assert!(coordinate_child_phases(&mut claim).is_err());
        assert_eq!(claim.compensation, ["stop", "handoff", "agent"]);
        assert!(claim.resources.is_empty());

        let mut dispatch = TestOps {
            fault: FaultStage::BeforeDispatch,
            ..TestOps::default()
        };
        assert!(coordinate_child_phases(&mut dispatch).is_err());
        assert_eq!(dispatch.compensation, ["stop", "terminal", "agent"]);
        assert_eq!(dispatch.resources, BTreeSet::from(["channel"]));
    }

    #[test]
    fn d_missing_grants_do_not_enter_transaction() {
        let mut ops = TestOps::default();
        assert!(coordinate_authorized_child(false, &mut ops).is_err());
        assert!(ops.resources.is_empty());
        assert!(ops.compensation.is_empty());
    }

    #[test]
    fn d_active_claim_precedes_durable_dispatch() {
        let mut ops = TestOps {
            fault: FaultStage::Ready,
            ..TestOps::default()
        };
        assert!(coordinate_child_phases(&mut ops).is_ok());
        assert_eq!(ops.phases, ["claim", "dispatch"]);
        assert_eq!(
            ops.resources,
            BTreeSet::from(["agent", "channel", "unit", "socket", "request"])
        );
        assert!(ops.compensation.is_empty());
    }

    #[test]
    fn d_handoff_dispatch_survives_the_create_call() {
        let source = std::env::temp_dir().join(format!(
            "cortexfs-child-handoff-dispatch-{}",
            std::process::id()
        ));
        let _removed = fs::remove_dir_all(&source);
        let agent = source.join("agent");
        assert!(fs::create_dir_all(&agent).is_ok());
        let socket = agent.join("worker.sock");
        let listener = UnixListener::bind(&socket).ok();
        assert!(listener.is_some());
        let Some(listener) = listener else { return };
        let session_root = source.join("home/1000/agent/worker/session");
        assert!(fs::create_dir_all(&session_root).is_ok());
        let server_session_root = session_root.clone();
        let server = std::thread::spawn(move || {
            let (mut stream, _address) = listener.accept().map_err(|error| error.to_string())?;
            let mut frame = String::new();
            stream
                .read_to_string(&mut frame)
                .map_err(|error| error.to_string())?;
            let response = crate::handle_socket_request_frame(
                &server_session_root,
                "/workspace",
                Some("main"),
                &frame,
            )
            .map_err(|error| format!("{error:?}"))?;
            let response_jsonl = response.jsonl();
            stream
                .write_all(response_jsonl.as_bytes())
                .map_err(|error| error.to_string())?;
            Ok::<(String, String), String>((frame, response_jsonl))
        });
        let dispatched = dispatch_child_handoff(
            &source,
            "worker",
            "worker-run",
            "/workspace",
            "Review the parent plan",
        );
        let exchange = server.join().ok().and_then(Result::ok);
        assert!(dispatched.is_ok(), "{dispatched:?}; exchange={exchange:?}");
        let frame = exchange.as_ref().map(|exchange| exchange.0.as_str());
        assert!(frame.is_some());
        let value =
            frame.and_then(|frame| serde_json::from_str::<serde_json::Value>(frame.trim()).ok());
        assert_eq!(
            value,
            Some(serde_json::json!({
                "op": "send",
                "id": "handoff-worker-run",
                "session": "worker-run",
                "scope": "private",
                "cwd": "/workspace",
                "input": "Review the parent plan",
            }))
        );
        let durable = session_root.join("worker-run");
        assert!(
            fs::read_to_string(durable.join("messages.jsonl"))
                .is_ok_and(|messages| messages.contains("Review the parent plan"))
        );
        assert!(
            fs::read_to_string(durable.join("events.jsonl"))
                .is_ok_and(|events| events.contains("handoff-worker-run"))
        );
        assert!(fs::remove_dir_all(source).is_ok());
    }

    #[test]
    fn d_handoff_dispatch_rejects_runtime_close_before_ack() {
        let source = std::env::temp_dir().join(format!(
            "cortexfs-child-handoff-reject-{}",
            std::process::id()
        ));
        let _removed = fs::remove_dir_all(&source);
        let agent = source.join("agent");
        assert!(fs::create_dir_all(&agent).is_ok());
        let listener = UnixListener::bind(agent.join("worker.sock")).ok();
        assert!(listener.is_some());
        let Some(listener) = listener else { return };
        let invalid_session_root = source.join("invalid-session-root");
        assert!(fs::write(&invalid_session_root, "not a directory").is_ok());
        let server = std::thread::spawn(move || {
            let accepted = listener.accept();
            assert!(accepted.is_ok());
            let Ok((mut stream, _address)) = accepted else {
                return;
            };
            let mut frame = String::new();
            assert!(stream.read_to_string(&mut frame).is_ok());
            assert!(
                crate::handle_socket_request_frame(
                    &invalid_session_root,
                    "/workspace",
                    Some("main"),
                    &frame,
                )
                .is_err()
            );
        });
        assert!(
            dispatch_child_handoff(
                &source,
                "worker",
                "worker-run",
                "/workspace",
                "Review the parent plan",
            )
            .is_err()
        );
        assert!(server.join().is_ok());
        assert!(fs::remove_dir_all(source).is_ok());
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "root lifecycle fixture remains one auditable transaction"
    )]
    fn d_root_isolated_parent_claim_failure_stops_before_dispatch_and_rollback() {
        if !nix::unistd::Uid::effective().is_root() {
            return;
        }
        let source = Path::new("/var/lib/cortexfs/storage/current");
        let random_id = fs::read_to_string("/proc/sys/kernel/random/uuid");
        assert!(random_id.is_ok());
        let Ok(random_id) = random_id else { return };
        let random_id = random_id.trim();
        assert_eq!(random_id.len(), 36);
        let fixture_id = random_id.get(..8);
        assert!(fixture_id.is_some());
        let Some(fixture_id) = fixture_id else { return };
        let parent = format!("claim-parent-{fixture_id}");
        let child = format!("claim-child-{fixture_id}");
        let session = format!("claim-session-{fixture_id}");
        let coder = crate::derive_agent_runtime_view(source, "coder").ok();
        assert!(coder.is_some());
        let Some(coder) = coder else { return };
        let parent_subject = format!("{parent}_t");
        let parent_label = format!("user_u:agent_r:{parent_subject}:s0");
        let policy = format!(
            "allow {parent_subject} tool:agent.create execute\nallow {parent_subject} agent:{child} create\nallow {parent_subject} agent:{child} start\nallow {parent_subject} model:debug/echo use\n"
        );
        let owner_uid = coder.identity().uid().to_string();
        let gid = coder.identity().gid().to_string();
        let groups = coder
            .identity()
            .groups()
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        let workspace = tempfile::tempdir();
        assert!(workspace.is_ok());
        let Ok(workspace) = workspace else { return };
        let mount = format!(
            "/ctx\t/ctx\tro\trbind,nosuid,nodev\n{}\t/workspace\trw\trbind,nosuid,nodev\n",
            workspace.path().display()
        );
        let overrides = [
            ("owner", owner_uid.as_str()),
            ("uid", owner_uid.as_str()),
            ("gid", gid.as_str()),
            ("groups", groups.as_str()),
            ("label", parent_label.as_str()),
            ("root", "/ctx"),
            ("cwd", "/workspace"),
            ("env", "CTX_ROOT=/ctx"),
            ("path", "/ctx/tool"),
            ("mount", mount.as_str()),
            ("model", "debug/echo"),
            ("policy", policy.as_str()),
            ("status", "idle"),
        ];
        let parent_receipt = crate::agent::create::create_agent_files(
            source,
            &owner_uid,
            &parent,
            &child_executable(&parent),
            &overrides,
        );
        assert!(parent_receipt.is_ok());
        let Ok(parent_receipt) = parent_receipt else {
            return;
        };
        let parent_sessions = source
            .join("home")
            .join(&owner_uid)
            .join("agent")
            .join(&parent)
            .join("session");
        let baseline_nodes = record_fixture_tree(&parent_sessions);
        assert!(baseline_nodes.is_ok());
        assert!(
            crate::ensure_durable_session_layout(
                &parent_sessions,
                "default",
                "/workspace",
                Some("debug/echo"),
                crate::SocketSessionScope::Private,
            )
            .is_ok()
        );
        let agent_root = source.join("agent");
        let home_agent_root = source.join("home").join(&owner_uid).join("agent");
        let agent_baseline = record_fixture_tree(&agent_root);
        let home_baseline = record_fixture_tree(&home_agent_root);
        assert!(agent_baseline.is_ok());
        assert!(home_baseline.is_ok());
        FORCE_PRODUCTION_CLAIM_CONFLICT.store(true, std::sync::atomic::Ordering::Release);
        let result = create_child_context(
            source,
            Path::new("/ctx"),
            &parent,
            "default",
            "claim-run",
            &child,
            Some(&session),
            None,
            None,
            "claim conflict",
            "temp",
        );
        assert!(
            matches!(result, Err(ref error) if error.message() == "forced child claim conflict"),
            "{result:?}"
        );
        assert!(!source.join("agent").join(&child).exists());
        assert!(!source.join("agent").join(format!("{child}.d")).exists());
        assert!(!home_agent_root.join(&child).exists());
        assert!(
            !parent_sessions
                .join("default/context/child")
                .join(&child)
                .exists()
        );
        for unit in [
            format!("cortexfs-agent@{child}.socket"),
            format!("cortexfs-agent-{child}-{session}-terminal.service"),
        ] {
            let output = std::process::Command::new(crate::support::command::SYSTEMCTL)
                .args(["show", "--property=ActiveState", "--value", &unit])
                .output();
            assert!(output.is_ok());
            assert!(output.is_ok_and(|output| {
                matches!(
                    String::from_utf8_lossy(&output.stdout).trim(),
                    "" | "inactive" | "failed"
                )
            }));
        }
        let fixture_nodes = record_fixture_tree(&parent_sessions);
        assert!(fixture_nodes.is_ok());
        let additions = fixture_additions(
            &baseline_nodes.unwrap_or_default(),
            fixture_nodes.unwrap_or_default(),
        );
        assert!(additions.is_ok());
        assert!(cleanup_fixture_tree(&parent_sessions, additions.unwrap_or_default()).is_ok());
        let child_agent_nodes = fixture_additions(
            &agent_baseline.unwrap_or_default(),
            record_fixture_tree(&agent_root).unwrap_or_default(),
        )
        .unwrap_or_default()
        .into_iter()
        .filter(|node| {
            node.path
                .components()
                .next()
                .and_then(|part| part.as_os_str().to_str())
                .is_some_and(|part| part == child || part == format!("{child}.d"))
        })
        .collect();
        let child_home_nodes = fixture_additions(
            &home_baseline.unwrap_or_default(),
            record_fixture_tree(&home_agent_root).unwrap_or_default(),
        )
        .unwrap_or_default()
        .into_iter()
        .filter(|node| node.path.starts_with(&child))
        .collect();
        assert!(cleanup_fixture_tree(&agent_root, child_agent_nodes).is_ok());
        assert!(cleanup_fixture_tree(&home_agent_root, child_home_nodes).is_ok());
        let parent_cleanup = crate::agent::create::rollback_agent_files(parent_receipt);
        assert!(parent_cleanup.is_ok(), "{parent_cleanup:?}");
        assert!(!source.join("agent").join(&parent).exists());
        assert!(!source.join("agent").join(&child).exists());
        assert!(!source.join("agent").join(format!("{child}.d")).exists());
        assert!(!home_agent_root.join(&child).exists());
    }

    #[test]
    fn d_compensation_conflict_has_priority_and_all_cleanup_is_attempted() {
        for (conflict, fault, expected, compensation, resources) in [
            (
                "stop",
                FaultStage::BeforeDispatch,
                "stop conflict",
                &["stop", "terminal", "agent"][..],
                BTreeSet::from(["channel"]),
            ),
            (
                "handoff",
                FaultStage::BeforeClaim,
                "handoff conflict",
                &["stop", "handoff", "agent"][..],
                BTreeSet::new(),
            ),
            (
                "agent",
                FaultStage::AfterLaunch,
                "agent conflict complete receipt",
                &["handoff", "agent"][..],
                BTreeSet::new(),
            ),
        ] {
            let mut ops = TestOps {
                fault,
                conflict: Some(conflict),
                ..TestOps::default()
            };
            let error = coordinate_child_phases(&mut ops).err();
            assert!(matches!(error, Some(error) if error.message() == expected));
            assert_eq!(ops.compensation, compensation);
            assert_eq!(ops.resources, resources);
        }
    }

    #[test]
    fn d_pre_materialization_session_layout_failure_rolls_back_agent() {
        let root = tempfile::tempdir();
        let Ok(root) = root else { return };
        let receipt = crate::agent::create::create_agent_files(
            root.path(),
            "1000",
            "child",
            "#!/bin/sh\nexit 0\n",
            &[],
        );
        let Ok(receipt) = receipt else { return };
        let session = root.path().join("home/1000/agent/child/session/dedicated");
        assert!(
            finish_session_layout(
                receipt,
                Err(crate::DurableSessionLayoutError::InvalidSessionName),
            )
            .is_err()
        );
        for path in [
            root.path().join("agent/child"),
            root.path().join("agent/child.d"),
            root.path().join("agent/child.sock"),
            root.path().join("home/1000/agent/child"),
            session,
        ] {
            assert!(!path.exists(), "orphan remained: {}", path.display());
        }
    }

    #[test]
    fn d_privileged_session_tree_repair_allows_target_prepare() {
        let root = tempfile::tempdir();
        let Ok(root) = root else { return };
        let session_root = root.path().join("session");
        assert!(
            crate::ensure_durable_session_layout(
                &session_root,
                "dedicated",
                "/workspace",
                Some("debug/echo"),
                crate::SocketSessionScope::Private,
            )
            .is_ok()
        );
        let uid = if nix::unistd::geteuid().is_root() {
            1000
        } else {
            nix::unistd::geteuid().as_raw()
        };
        let gid = if nix::unistd::geteuid().is_root() {
            1000
        } else {
            nix::unistd::getegid().as_raw()
        };
        assert!(
            crate::runtime::record::session::repair_agent_session_permissions(
                &session_root,
                uid,
                gid,
            )
            .is_ok()
        );
        for (path, mode) in [
            (session_root.clone(), 0o700),
            (session_root.join("index"), 0o700),
            (session_root.join("index/list"), 0o600),
            (session_root.join("dedicated"), 0o700),
        ] {
            let metadata = fs::metadata(path);
            assert!(metadata.is_ok_and(|metadata| {
                metadata.uid() == uid
                    && metadata.gid() == gid
                    && metadata.permissions().mode() & 0o777 == mode
            }));
        }
        assert!(
            crate::runtime::record::session::prepare_owned_durable_session(
                &session_root,
                "dedicated",
                "/workspace",
                Some("debug/echo"),
                crate::SocketSessionScope::Private,
                uid,
                gid,
            )
            .is_ok()
        );
    }

    #[test]
    fn d_privileged_handoff_is_owner_readable() {
        let root = tempfile::tempdir();
        let Ok(root) = root else { return };
        let parent_session = root.path().join("parent/session/default");
        let parent_root = root.path().join("parent/session");
        assert!(
            crate::ensure_durable_session_layout(
                &parent_root,
                "default",
                "/",
                None,
                crate::SocketSessionScope::Private,
            )
            .is_ok()
        );
        let uid = nix::unistd::geteuid().as_raw();
        let gid = nix::unistd::getegid().as_raw();
        let agent = crate::agent::create::create_agent_files(
            root.path(),
            "1000",
            "placeholder",
            "#!/bin/sh\n",
            &[],
        );
        let Ok(agent) = agent else { return };
        let mut ops = ProductionOps {
            source: root.path(),
            uid: "1000",
            owner_uid: uid,
            owner_gid: gid,
            name: "child",
            executable: "",
            overrides: Vec::new(),
            child_session_root: root.path().join("unused"),
            child_session: "child-session",
            cwd: "/",
            model: "debug/echo",
            parent_session,
            handoff: "run",
        };
        let receipt = ops.publish_handoff(&agent);
        assert!(receipt.is_ok());
        let channel = root
            .path()
            .join("parent/session/default/context/child/child");
        assert!(fs::read_to_string(channel.join("handoff.md")).is_ok());
        assert!(fs::metadata(channel).is_ok_and(|metadata| {
            metadata.uid() == uid
                && metadata.gid() == gid
                && metadata.permissions().mode() & 0o777 == 0o700
        }));
    }

    #[test]
    fn d_handoff_permission_failure_rolls_back_channel() {
        let root = tempfile::tempdir();
        let Ok(root) = root else { return };
        let parent_root = root.path().join("parent/session");
        assert!(
            crate::ensure_durable_session_layout(
                &parent_root,
                "default",
                "/",
                None,
                crate::SocketSessionScope::Private,
            )
            .is_ok()
        );
        let parent_session = parent_root.join("default");
        let invalid = UnixListener::bind(parent_session.join("context/invalid.sock"));
        assert!(invalid.is_ok());
        let agent = crate::agent::create::create_agent_files(
            root.path(),
            "1000",
            "placeholder",
            "#!/bin/sh\n",
            &[],
        );
        let Ok(agent) = agent else { return };
        let mut ops = ProductionOps {
            source: root.path(),
            uid: "1000",
            owner_uid: nix::unistd::geteuid().as_raw(),
            owner_gid: nix::unistd::getegid().as_raw(),
            name: "child",
            executable: "",
            overrides: Vec::new(),
            child_session_root: root.path().join("unused"),
            child_session: "child-session",
            cwd: "/",
            model: "debug/echo",
            parent_session: parent_session.clone(),
            handoff: "run",
        };
        let result = ops.publish_handoff(&agent);
        assert!(result.is_err());
        assert!(!parent_session.join("context/child/child").exists());
    }

    #[test]
    fn d_partial_session_layout_failure_rolls_back_created_paths_only() {
        let root = tempfile::tempdir();
        let Ok(root) = root else { return };
        let receipt = crate::agent::create::create_agent_files(
            root.path(),
            "1000",
            "child",
            "#!/bin/sh\nexit 0\n",
            &[],
        );
        let Ok(receipt) = receipt else { return };
        let session_root = root.path().join("home/1000/agent/child/session");
        assert!(fs::create_dir_all(session_root.join("index/list")).is_ok());
        let result = crate::ensure_durable_session_layout(
            &session_root,
            "dedicated",
            "/workspace",
            Some("debug/echo"),
            crate::SocketSessionScope::Private,
        );
        assert_eq!(result, Err(crate::DurableSessionLayoutError::CannotCreate));
        assert!(!session_root.join("dedicated").exists());
        assert!(session_root.join("index/list").is_dir());
        let error = finish_session_layout(receipt, result).err();
        assert!(error.is_some());
        let Some(error) = error else { return };
        assert!(error.message().contains("CannotCreate"));
        assert!(!root.path().join("agent/child").exists());
        assert!(!session_root.join("dedicated").exists());
        let quarantines = fs::read_dir(root.path().join("agent"))
            .ok()
            .into_iter()
            .flatten()
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".ctx-rollback-")
            });
        assert!(!quarantines);
    }

    #[test]
    fn d_child_stop_is_terminal_first_and_fail_closed() {
        use crate::agent::launch::AgentLaunchError;
        let trace = std::cell::RefCell::new(Vec::new());
        let result = stop_child_with(
            || {
                trace.borrow_mut().push("terminal");
                Err(AgentLaunchError::StopConflict)
            },
            || {
                trace.borrow_mut().push("system");
                Ok(())
            },
        );
        assert!(result.is_err());
        assert_eq!(*trace.borrow(), ["terminal"]);

        trace.borrow_mut().clear();
        let result = stop_child_with(
            || {
                trace.borrow_mut().push("terminal");
                Ok(())
            },
            || {
                trace.borrow_mut().push("system");
                Err(AgentLaunchError::StopConflict)
            },
        );
        assert!(result.is_err());
        assert_eq!(*trace.borrow(), ["terminal", "system"]);
    }

    #[test]
    fn d_complete_agent_rollback_conflict_survives_sdk_mapping() {
        let conflict = crate::agent::create::AgentRollbackConflict {
            original: "/ctx/agent/child.d".into(),
            quarantine: Some("/ctx/agent/.ctx-rollback-9".into()),
            dev: 17,
            ino: 23,
            stage: "original-recreated",
        };
        let error = create_error(crate::agent::create::AgentCreateError::RollbackConflict(
            conflict,
        ));
        assert_eq!(error.code(), "EIO");
        for detail in [
            "original=/ctx/agent/child.d",
            "quarantine=/ctx/agent/.ctx-rollback-9",
            "dev=17",
            "ino=23",
            "stage=original-recreated",
        ] {
            assert!(error.message().contains(detail));
        }
    }

    #[test]
    fn runtime_receipt_merge_preserves_existing_agent_meta() {
        let root = tempfile::tempdir();
        let Ok(root) = root else { return };
        let control = root.path().join("agent/child.d");
        assert!(fs::create_dir_all(&control).is_ok());
        assert!(fs::write(control.join("meta.json"), "{\"description\":\"kept\"}\n").is_ok());
        let launch = ChildLaunch {
            terminal: crate::agent::launch::AgentLaunchReceipt {
                unit: "terminal-unit".to_owned(),
                pid: 42,
                identity: crate::AgentUnixIdentity::new(1000, 1000, [10, 20]),
                invocation: "terminal-invocation".to_owned(),
                socket: PathBuf::from("/run/user/1000/terminal.sock"),
            },
            system: crate::agent::launch::SystemAgentSocketReceipt {
                unit: "system-unit".to_owned(),
                was_active: false,
                owned_start: true,
                invocation: "system-invocation".to_owned(),
            },
            pid: 42,
            session: "child-session".to_owned(),
        };
        assert!(
            crate::agent::launch::persist_agent_launch_meta(
                root.path(),
                "child",
                &launch.terminal,
                &launch.system,
            )
            .is_ok()
        );
        let content = fs::read_to_string(control.join("meta.json")).ok();
        let value = content
            .as_deref()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(content).ok());
        assert_eq!(
            value
                .as_ref()
                .and_then(|value| value.get("description"))
                .and_then(serde_json::Value::as_str),
            Some("kept")
        );
        let receipt = value
            .as_ref()
            .and_then(|value| value.get("runtime_receipt"));
        assert_eq!(
            receipt
                .and_then(|receipt| receipt.pointer("/terminal/pid"))
                .and_then(serde_json::Value::as_u64),
            Some(42)
        );
        assert_eq!(
            receipt
                .and_then(|receipt| receipt.pointer("/system/owned_start"))
                .and_then(serde_json::Value::as_bool),
            Some(true)
        );
    }

    #[test]
    fn startup_stub_accepts_valid_retry_and_rejects_hardlink() -> io::Result<()> {
        let root = tempfile::tempdir()?;
        let parent = crate::support::plain::open_plain_directory(root.path())?;
        let uid = nix::unistd::geteuid().as_raw();
        let gid = nix::unistd::getegid().as_raw();
        let created = prepare_startup_stub(&parent, uid, gid)?;
        assert!(created.created);
        let retry = prepare_startup_stub(&parent, uid, gid)?;
        assert!(!retry.created);
        fs::hard_link(
            root.path().join(".empty-shell-startup"),
            root.path().join("alias"),
        )?;
        assert!(prepare_startup_stub(&parent, uid, gid).is_err());
        assert!(cleanup_created_startup_stub(&parent, created).is_err());
        assert!(root.path().join(".empty-shell-startup").exists());
        Ok(())
    }
}
