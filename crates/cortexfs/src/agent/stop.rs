use crate::agent::launch::{
    AgentLaunchError, AgentLaunchReceipt, SystemAgentSocketReceipt, stop_launch,
    stop_system_agent_socket,
};
use crate::support::{columnar, plain::open_plain_directory};
use crate::{ChildHandoffReceipt, agent::runtime::AgentUnixIdentity};
use nix::libc;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::PathBuf;

/// Authority and backing-layout inputs for stop preparation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StopContext {
    pub source: PathBuf,
    pub owner_uid: u32,
    pub peer_uid: u32,
    pub runtime_agent: String,
}

/// Domain mutations emitted by stop preparation.
#[derive(Debug)]
pub enum StopAction {
    CancelOwnedChannel(PlannedCancellation),
    StopRuntime(PlannedRuntimeStop),
    CleanupTemp(TempCleanupPlan),
    StopSelf(PlannedStop),
}

#[derive(Debug)]
pub struct StopFileReceipt {
    pub path: PathBuf,
    pub dev: u64,
    pub ino: u64,
}

#[derive(Debug)]
pub struct StopControlReceipts {
    pub status: StopFileReceipt,
    pub pid: StopFileReceipt,
    pub log: StopFileReceipt,
}

#[derive(Debug)]
pub struct PlannedRuntimeStop {
    pub terminal: AgentLaunchReceipt,
    pub system: SystemAgentSocketReceipt,
    pub terminal_live: bool,
    pub system_live: bool,
}

#[derive(Debug)]
pub struct CancellationHistoryReceipts {
    pub parent_events: StopFileReceipt,
    pub child_messages: StopFileReceipt,
    pub child_events: StopFileReceipt,
    pub child_state: StopFileReceipt,
}

#[derive(Debug)]
pub struct PlannedCancellation {
    pub parent_agent: String,
    pub parent_session: PathBuf,
    pub child_agent: String,
    pub child_session: String,
    pub child_session_dir: PathBuf,
    pub receipt: ChildHandoffReceipt,
    pub lease: crate::runtime::record::child::ChildFinishLease,
    pub record_events: bool,
    pub history: Option<CancellationHistoryReceipts>,
}

#[derive(Debug)]
pub struct TempCleanupPlan {
    pub entries: Vec<TempCleanupEntry>,
}

#[derive(Debug)]
pub struct TempCleanupEntry {
    pub path: PathBuf,
    pub directory: bool,
    pub dev: u64,
    pub ino: u64,
    pub kind: u32,
}

#[derive(Debug)]
pub struct PlannedStop {
    pub name: String,
    pub identity: Option<AgentUnixIdentity>,
    pub stop_system_socket: bool,
    pub terminal_units: Vec<String>,
    pub cancellations: Vec<PlannedCancellation>,
    pub cleanup: Option<TempCleanupPlan>,
    pub runtime: Option<PlannedRuntimeStop>,
    pub control: StopControlReceipts,
}

pub fn bind_file(path: &std::path::Path, write: bool) -> Result<StopFileReceipt, StopError> {
    let parent = path
        .parent()
        .ok_or_else(|| StopError::new("stop receipt path has no parent"))?;
    let directory = open_plain_directory(parent)
        .map_err(|error| StopError::new(format!("cannot open {}: {error}", parent.display())))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| StopError::new("invalid stop receipt path"))?;
    let flags = if write {
        nix::fcntl::OFlag::O_WRONLY
    } else {
        nix::fcntl::OFlag::O_RDONLY
    } | nix::fcntl::OFlag::O_NOFOLLOW
        | nix::fcntl::OFlag::O_CLOEXEC;
    let file = nix::fcntl::openat(&directory, name, flags, nix::sys::stat::Mode::empty())
        .map(fs::File::from)
        .map_err(|error| StopError::new(format!("cannot bind {}: {error}", path.display())))?;
    let metadata = file
        .metadata()
        .map_err(|error| StopError::new(format!("cannot stat {}: {error}", path.display())))?;
    if !metadata.is_file() {
        return Err(StopError::new("stop receipt is not a plain file"));
    }
    Ok(StopFileReceipt {
        path: path.to_owned(),
        dev: metadata.dev(),
        ino: metadata.ino(),
    })
}

pub fn bind_control(control: &std::path::Path) -> Result<StopControlReceipts, StopError> {
    Ok(StopControlReceipts {
        status: bind_file(&control.join("status"), true)?,
        pid: bind_file(&control.join("pid"), true)?,
        log: bind_file(&control.join("log"), true)?,
    })
}

fn open_file(receipt: &StopFileReceipt, append: bool) -> Result<fs::File, StopError> {
    let mut options = fs::OpenOptions::new();
    options
        .write(true)
        .append(append)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    let file = options.open(&receipt.path).map_err(|error| {
        StopError::new(format!("cannot reopen {}: {error}", receipt.path.display()))
    })?;
    let metadata = file.metadata().map_err(|error| {
        StopError::new(format!("cannot stat {}: {error}", receipt.path.display()))
    })?;
    if !metadata.is_file() || (metadata.dev(), metadata.ino()) != (receipt.dev, receipt.ino) {
        return Err(StopError::new(format!(
            "stop receipt conflict: {}",
            receipt.path.display()
        )));
    }
    Ok(file)
}

pub fn verify_file(receipt: &StopFileReceipt, append: bool) -> Result<(), StopError> {
    open_file(receipt, append).map(drop)
}

pub fn verify_read_file(receipt: &StopFileReceipt) -> Result<(), StopError> {
    let current = bind_file(&receipt.path, false)?;
    if (current.dev, current.ino) != (receipt.dev, receipt.ino) {
        return Err(StopError::new(format!(
            "stop receipt conflict: {}",
            receipt.path.display()
        )));
    }
    Ok(())
}

pub fn replace_file(receipt: &StopFileReceipt, content: &str) -> Result<(), StopError> {
    let mut file = open_file(receipt, false)?;
    file.set_len(0)
        .and_then(|()| file.write_all(content.as_bytes()))
        .and_then(|()| file.sync_all())
        .map_err(|error| {
            StopError::new(format!("cannot write {}: {error}", receipt.path.display()))
        })
}

pub fn append_file(receipt: &StopFileReceipt, line: &str) -> Result<(), StopError> {
    let mut file = open_file(receipt, true)?;
    file.write_all(line.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|error| {
            StopError::new(format!("cannot append {}: {error}", receipt.path.display()))
        })
}

fn append_history_file(
    receipt: &StopFileReceipt,
    session: &std::path::Path,
    line: &str,
) -> Result<(), StopError> {
    let history = columnar::HistoryGuard::exclusive(session).map_err(|error| {
        StopError::new(format!("cannot lock {}: {error}", receipt.path.display()))
    })?;
    verify_file(receipt, true)?;
    history
        .refresh_claims()
        .and_then(|()| history.append(columnar::Stream::Events, &[line]))
        .and_then(|()| history.refresh_claims())
        .map_err(|error| {
            StopError::new(format!("cannot append {}: {error}", receipt.path.display()))
        })
}

/// Rejects requests outside the runtime's bound owner and agent before I/O.
pub fn authorize(context: &StopContext, requested_agent: &str) -> Result<(), StopError> {
    if context.peer_uid != context.owner_uid {
        return Err(StopError::new("agent stop peer denied"));
    }
    if requested_agent != context.runtime_agent {
        return Err(StopError::new("agent stop runtime agent mismatch"));
    }
    Ok(())
}

#[derive(Debug)]
struct Candidate {
    parent: Option<StopParent>,
    lifecycle: crate::ChildLifecycle,
}

#[derive(Clone, Debug)]
struct StopParent {
    agent: String,
    session: Option<String>,
}

/// Enumerates the requested agent and its owned descendants in stop order.
pub fn ordered_owned_agents(
    context: &StopContext,
    requested_agent: &str,
) -> Result<Vec<String>, StopError> {
    authorize(context, requested_agent)?;
    let agent_root = context.source.join("agent");
    let entries = fs::read_dir(&agent_root).map_err(|error| {
        StopError::new(format!("cannot read {}: {error}", agent_root.display()))
    })?;
    let mut candidates = HashMap::new();
    for entry in entries {
        let entry = entry
            .map_err(|error| StopError::new(format!("cannot read agent controls: {error}")))?;
        let Some(name) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.strip_suffix(".d"))
            .map(str::to_owned)
        else {
            continue;
        };
        if !crate::is_object_name(&name) {
            continue;
        }
        let control = entry.path();
        let life = read_trimmed(&control.join("life"))?.unwrap_or_else(|| "owned".to_owned());
        let lifecycle = crate::ChildLifecycle::parse(&life)
            .map_err(|_error| StopError::new(format!("invalid agent life: {name}")))?;
        let parent = read_trimmed(&control.join("parent"))?
            .filter(|value| !value.is_empty())
            .map(|value| parse_parent(&value))
            .transpose()
            .map_err(|_error| StopError::new(format!("invalid agent parent: {name}")))?;
        candidates.insert(name, Candidate { parent, lifecycle });
    }
    if !candidates.contains_key(requested_agent) {
        return Err(StopError::new("missing agent control"));
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut ordered = Vec::new();
    order_agent(
        requested_agent,
        &candidates,
        &mut visiting,
        &mut visited,
        &mut ordered,
    )?;
    Ok(ordered)
}

pub fn parse_runtime_stop_receipt(
    control: &std::path::Path,
) -> Result<PlannedRuntimeStop, StopError> {
    let control_meta = fs::symlink_metadata(control)
        .map_err(|error| StopError::new(format!("cannot stat {}: {error}", control.display())))?;
    let receipt_path = control.join("meta.json");
    let receipt_file = fs::File::open(&receipt_path)
        .map_err(|error| StopError::new(format!("cannot read runtime receipt: {error}")))?;
    let content = crate::support::process::read_limited_text(receipt_file, 65_536);
    let value: serde_json::Value = serde_json::from_str(&content)
        .map_err(|_error| StopError::new("invalid runtime receipt"))?;
    let receipt = value
        .get("runtime_receipt")
        .ok_or_else(|| StopError::new("missing runtime receipt"))?;
    require_receipt_keys(receipt, &["version", "control", "terminal", "system"])?;
    if receipt.get("version").and_then(serde_json::Value::as_u64) != Some(1) {
        return Err(StopError::new("invalid runtime receipt version"));
    }
    let control_receipt = receipt
        .get("control")
        .ok_or_else(|| StopError::new("missing control receipt"))?;
    let number = |object: &serde_json::Value, field: &str| {
        object
            .get(field)
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| StopError::new(format!("invalid runtime receipt {field}")))
    };
    if (
        number(control_receipt, "dev")?,
        number(control_receipt, "ino")?,
    ) != (control_meta.dev(), control_meta.ino())
    {
        return Err(StopError::new("runtime control receipt conflict"));
    }
    require_receipt_keys(control_receipt, &["dev", "ino"])?;
    let terminal = receipt
        .get("terminal")
        .ok_or_else(|| StopError::new("missing terminal receipt"))?;
    require_receipt_keys(
        terminal,
        &["session", "unit", "invocation", "pid", "identity"],
    )?;
    let string = |object: &serde_json::Value, field: &str| {
        object
            .get(field)
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| StopError::new(format!("invalid runtime receipt {field}")))
    };
    if !crate::is_object_name(&string(terminal, "session")?) {
        return Err(StopError::new("invalid runtime session"));
    }
    let identity = terminal
        .get("identity")
        .ok_or_else(|| StopError::new("missing runtime identity"))?;
    require_receipt_keys(identity, &["uid", "gid", "groups"])?;
    let uid = u32::try_from(number(identity, "uid")?)
        .map_err(|_error| StopError::new("invalid runtime uid"))?;
    let gid = u32::try_from(number(identity, "gid")?)
        .map_err(|_error| StopError::new("invalid runtime gid"))?;
    let groups = identity
        .get("groups")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| StopError::new("invalid runtime groups"))?
        .iter()
        .map(|group| group.as_u64().and_then(|group| u32::try_from(group).ok()))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| StopError::new("invalid runtime groups"))?;
    let system = receipt
        .get("system")
        .ok_or_else(|| StopError::new("missing system receipt"))?;
    require_receipt_keys(system, &["unit", "invocation", "owned_start"])?;
    let owned_start = system
        .get("owned_start")
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| StopError::new("invalid owned_start receipt"))?;
    let terminal = AgentLaunchReceipt {
        unit: string(terminal, "unit")?,
        pid: u32::try_from(number(terminal, "pid")?)
            .map_err(|_error| StopError::new("invalid runtime pid"))?,
        identity: AgentUnixIdentity::new(uid, gid, groups),
        invocation: string(terminal, "invocation")?,
        socket: PathBuf::new(),
    };
    let system = SystemAgentSocketReceipt {
        unit: string(system, "unit")?,
        was_active: !owned_start,
        owned_start,
        invocation: string(system, "invocation")?,
    };
    let terminal_live = crate::agent::launch::verify_launch(&terminal)
        .map_err(|_error| StopError::new("terminal receipt conflict"))?;
    let system_live = crate::agent::launch::verify_system_agent_socket(&system)
        .map_err(|_error| StopError::new("system receipt conflict"))?;
    Ok(PlannedRuntimeStop {
        terminal,
        system,
        terminal_live,
        system_live,
    })
}

fn require_receipt_keys(value: &serde_json::Value, expected: &[&str]) -> Result<(), StopError> {
    let object = value
        .as_object()
        .ok_or_else(|| StopError::new("runtime receipt field is not an object"))?;
    if object.len() != expected.len() || expected.iter().any(|key| !object.contains_key(*key)) {
        return Err(StopError::new("runtime receipt has invalid fields"));
    }
    Ok(())
}

pub fn plan_temp_cleanup(context: &StopContext, name: &str) -> Result<TempCleanupPlan, StopError> {
    let agent_root = context.source.join("agent");
    preflight_cleanup_directory(&agent_root, context.owner_uid)?;
    let mut entries = Vec::new();
    for path in [
        agent_root.join(name),
        agent_root.join(format!("{name}.sock")),
    ] {
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_dir() => {
                return Err(StopError::new(format!(
                    "temp agent path is not a file or socket: {}",
                    path.display()
                )));
            }
            Ok(metadata) => entries.push(TempCleanupEntry {
                path,
                directory: false,
                dev: metadata.dev(),
                ino: metadata.ino(),
                kind: metadata.mode() & libc::S_IFMT,
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(StopError::new(format!(
                    "cannot stat {}: {error}",
                    path.display()
                )));
            }
        }
    }
    plan_temp_cleanup_tree(
        &agent_root.join(format!("{name}.d")),
        context.owner_uid,
        &mut entries,
    )?;
    Ok(TempCleanupPlan { entries })
}

fn plan_temp_cleanup_tree(
    directory: &std::path::Path,
    owner_uid: u32,
    entries: &mut Vec<TempCleanupEntry>,
) -> Result<(), StopError> {
    preflight_cleanup_directory(directory, owner_uid)?;
    let mut children = fs::read_dir(directory)
        .map_err(|error| StopError::new(format!("cannot read {}: {error}", directory.display())))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|error| StopError::new(format!("cannot read directory: {error}")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    children.sort();
    for path in children {
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| StopError::new(format!("cannot stat {}: {error}", path.display())))?;
        if metadata.file_type().is_dir() {
            plan_temp_cleanup_tree(&path, owner_uid, entries)?;
        } else {
            entries.push(TempCleanupEntry {
                path,
                directory: false,
                dev: metadata.dev(),
                ino: metadata.ino(),
                kind: metadata.mode() & libc::S_IFMT,
            });
        }
    }
    let metadata = fs::symlink_metadata(directory)
        .map_err(|error| StopError::new(format!("cannot stat {}: {error}", directory.display())))?;
    entries.push(TempCleanupEntry {
        path: directory.to_owned(),
        directory: true,
        dev: metadata.dev(),
        ino: metadata.ino(),
        kind: metadata.mode() & libc::S_IFMT,
    });
    Ok(())
}

fn preflight_cleanup_directory(path: &std::path::Path, owner_uid: u32) -> Result<(), StopError> {
    use std::os::unix::fs::PermissionsExt;
    let directory = open_plain_directory(path)
        .map_err(|error| StopError::new(format!("cannot open {}: {error}", path.display())))?;
    let metadata = directory
        .metadata()
        .map_err(|error| StopError::new(format!("cannot stat {}: {error}", path.display())))?;
    let fuse = crate::support::plain::is_fuse(&directory).map_err(|error| {
        StopError::new(format!(
            "cannot inspect filesystem for {}: {error}",
            path.display()
        ))
    })?;
    if owner_uid != 0
        && !fuse
        && (metadata.uid() != owner_uid || metadata.permissions().mode() & 0o300 != 0o300)
    {
        return Err(StopError::new(format!(
            "temp cleanup directory is not owner-writable: {}",
            path.display()
        )));
    }
    Ok(())
}

pub fn execute_temp_cleanup(plan: TempCleanupPlan) -> Result<(), StopError> {
    for entry in plan.entries {
        verify_temp_cleanup_entry(&entry)?;
        let result = if entry.directory {
            fs::remove_dir(&entry.path)
        } else {
            fs::remove_file(&entry.path)
        };
        result.map_err(|error| {
            StopError::new(format!("cannot remove {}: {error}", entry.path.display()))
        })?;
    }
    Ok(())
}

fn verify_temp_cleanup_entry(entry: &TempCleanupEntry) -> Result<(), StopError> {
    let metadata = fs::symlink_metadata(&entry.path).map_err(|error| {
        StopError::new(format!("cannot stat {}: {error}", entry.path.display()))
    })?;
    if (
        metadata.dev(),
        metadata.ino(),
        metadata.mode() & libc::S_IFMT,
    ) != (entry.dev, entry.ino, entry.kind)
        || metadata.file_type().is_dir() != entry.directory
    {
        return Err(StopError::new(format!(
            "temp cleanup receipt conflict: {}",
            entry.path.display()
        )));
    }
    Ok(())
}

fn context_home(context: &StopContext) -> PathBuf {
    context
        .source
        .join("home")
        .join(context.owner_uid.to_string())
}

fn read_directory_names(path: &std::path::Path) -> Result<Vec<String>, StopError> {
    let mut names = fs::read_dir(path)
        .map_err(|error| StopError::new(format!("cannot read {}: {error}", path.display())))?
        .map(|entry| {
            entry
                .map_err(|error| StopError::new(format!("cannot read directory: {error}")))
                .and_then(|entry| {
                    entry
                        .file_name()
                        .into_string()
                        .map_err(|_name| StopError::new("invalid directory entry name"))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    names.sort();
    Ok(names)
}

fn preflight_writable_plain_file(path: &std::path::Path) -> Result<(), StopError> {
    let parent = path
        .parent()
        .ok_or_else(|| StopError::new("stop path has no parent"))?;
    let directory = open_plain_directory(parent)
        .map_err(|error| StopError::new(format!("cannot open {}: {error}", parent.display())))?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| StopError::new(format!("invalid stop path: {}", path.display())))?;
    let file = nix::fcntl::openat(
        &directory,
        name,
        nix::fcntl::OFlag::O_WRONLY | nix::fcntl::OFlag::O_NOFOLLOW | nix::fcntl::OFlag::O_CLOEXEC,
        nix::sys::stat::Mode::empty(),
    )
    .map(fs::File::from)
    .map_err(|error| StopError::new(format!("cannot write {}: {error}", path.display())))?;
    if !file
        .metadata()
        .map_err(|error| StopError::new(format!("cannot stat {}: {error}", path.display())))?
        .is_file()
    {
        return Err(StopError::new(format!(
            "stop path is not a plain file: {}",
            path.display()
        )));
    }
    Ok(())
}

fn preflight_child_cancellation(child: &std::path::Path) -> Result<(), StopError> {
    open_plain_directory(child)
        .map_err(|error| StopError::new(format!("cannot open {}: {error}", child.display())))?;
    for file in crate::CHILD_RESULT_REQUIRED_FILES {
        let path = child.join(file);
        let opened = fs::File::open(&path)
            .map_err(|error| StopError::new(format!("cannot open {}: {error}", path.display())))?;
        if !opened
            .metadata()
            .map_err(|error| StopError::new(format!("cannot stat {}: {error}", path.display())))?
            .is_file()
        {
            return Err(StopError::new(format!(
                "child result path is not a plain file: {}",
                path.display()
            )));
        }
    }
    for directory in crate::CHILD_RESULT_REQUIRED_DIRS {
        open_plain_directory(&child.join(directory)).map_err(|error| {
            StopError::new(format!(
                "cannot open {}: {error}",
                child.join(directory).display()
            ))
        })?;
    }
    for file in ["status", "result.md", "refs.jsonl"] {
        preflight_writable_plain_file(&child.join(file))?;
    }
    Ok(())
}

fn plan_parent_child_cancellations(
    context: &StopContext,
    child_agent: &str,
    parent: &StopParent,
) -> Result<Vec<PlannedCancellation>, StopError> {
    let session_root = context_home(context)
        .join("agent")
        .join(&parent.agent)
        .join("session");
    let parent_sessions = if let Some(session) = parent.session.as_deref() {
        vec![session_root.join(session)]
    } else if session_root
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.is_dir())
    {
        read_directory_names(&session_root)?
            .into_iter()
            .filter(|name| crate::is_object_name(name))
            .map(|name| session_root.join(name))
            .collect()
    } else {
        Vec::new()
    };
    let mut cancellations = Vec::new();
    for parent_session in parent_sessions {
        let child_root = parent_session.join("context/child");
        if !child_root
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.is_dir())
        {
            continue;
        }
        for child in read_directory_names(&child_root)? {
            let channel = child_root.join(child);
            if !channel
                .symlink_metadata()
                .is_ok_and(|metadata| metadata.is_dir())
                || read_trimmed(&channel.join("agent"))?.as_deref() != Some(child_agent)
            {
                continue;
            }
            let status = read_trimmed(&channel.join("status"))?.unwrap_or_default();
            if !matches!(
                crate::ChildContextStatus::parse(&status),
                Some(crate::ChildContextStatus::Pending | crate::ChildContextStatus::Active)
            ) {
                continue;
            }
            preflight_child_cancellation(&channel)?;
            let child_session = read_trimmed(&channel.join("session"))?
                .ok_or_else(|| StopError::new("missing child session"))?;
            let child_session_dir = context_home(context)
                .join("agent")
                .join(child_agent)
                .join("session")
                .join(&child_session);
            for path in [
                parent_session.join("events.jsonl"),
                child_session_dir.join("events.jsonl"),
                child_session_dir.join("state"),
            ] {
                preflight_writable_plain_file(&path)?;
            }
            let history = CancellationHistoryReceipts {
                parent_events: bind_file(&parent_session.join("events.jsonl"), true)?,
                child_messages: bind_file(&child_session_dir.join("messages.jsonl"), false)?,
                child_events: bind_file(&child_session_dir.join("events.jsonl"), true)?,
                child_state: bind_file(&child_session_dir.join("state"), true)?,
            };
            let receipt = crate::runtime::record::child_handoff_receipt(&channel)
                .map_err(|_error| StopError::new("cannot bind child handoff"))?;
            let lease = crate::runtime::record::child::acquire_child_finish_lease(&receipt)
                .map_err(|_error| StopError::new("cannot lock child handoff"))?;
            let locked_status = crate::runtime::record::child::child_finish_lease_status(&lease)
                .map_err(|_error| StopError::new("cannot inspect child handoff"))?;
            if matches!(
                locked_status,
                crate::ChildContextStatus::Done
                    | crate::ChildContextStatus::Error
                    | crate::ChildContextStatus::Cancelled
            ) {
                continue;
            }
            cancellations.push(PlannedCancellation {
                parent_agent: parent.agent.clone(),
                parent_session: parent_session.clone(),
                child_agent: child_agent.to_owned(),
                child_session,
                child_session_dir,
                receipt,
                lease,
                record_events: true,
                history: Some(history),
            });
        }
    }
    Ok(cancellations)
}

pub type ConcreteStopPlan = StopPlan<PlannedStop>;

/// Builds a complete receipt-bound stop plan without mutating runtime state.
pub fn plan_stop(
    context: &StopContext,
    requested_agent: &str,
) -> Result<ConcreteStopPlan, StopError> {
    let ordered = ordered_owned_agents(context, requested_agent)?;
    let mut entries = Vec::with_capacity(ordered.len());
    for name in ordered {
        let control = context.source.join("agent").join(format!("{name}.d"));
        let life = read_trimmed(&control.join("life"))?.unwrap_or_else(|| "owned".to_owned());
        let temporary = life == "temp";
        let parent = read_trimmed(&control.join("parent"))?
            .filter(|value| !value.is_empty())
            .map(|value| parse_parent(&value))
            .transpose()?;
        let runtime = parse_runtime_stop_receipt(&control)
            .map_err(|error| StopError::new(format!("{name}: {error}")))?;
        let cancellations = parent.as_ref().map_or_else(
            || Ok(Vec::new()),
            |parent| plan_parent_child_cancellations(context, &name, parent),
        )?;
        let cleanup = if temporary {
            Some(plan_temp_cleanup(context, &name)?)
        } else {
            None
        };
        entries.push(PlannedStop {
            name,
            identity: None,
            stop_system_socket: false,
            terminal_units: Vec::new(),
            cancellations,
            cleanup,
            runtime: Some(runtime),
            control: bind_control(&control)?,
        });
    }
    Ok(StopPlan::new(entries))
}

fn preflight_concrete(plan: &ConcreteStopPlan) -> Result<(), StopError> {
    for agent in plan.entries() {
        verify_file(&agent.control.status, false)?;
        verify_file(&agent.control.pid, false)?;
        verify_file(&agent.control.log, true)?;
        for cancellation in &agent.cancellations {
            preflight_child_cancellation(cancellation.receipt.path())?;
            let history = cancellation
                .history
                .as_ref()
                .ok_or_else(|| StopError::new("stop plan omitted cancellation history receipts"))?;
            verify_file(&history.parent_events, true)?;
            verify_read_file(&history.child_messages)?;
            verify_file(&history.child_events, true)?;
            verify_file(&history.child_state, false)?;
        }
        agent.cleanup.as_ref().map_or(Ok(()), |cleanup| {
            for entry in &cleanup.entries {
                verify_temp_cleanup_entry(entry)?;
            }
            Ok(())
        })?;
    }
    Ok(())
}

fn stop_concrete_agent(agent: PlannedStop) -> Result<(), StopError> {
    for cancellation in agent.cancellations {
        let cancelled = match crate::runtime::record::child::finish_child_result_with_lease(
            cancellation.lease,
            &cancellation.receipt,
            &cancellation.child_agent,
            &cancellation.child_session,
            crate::ChildContextStatus::Cancelled,
            &format!(
                "Child agent `{}` cancelled because the parent agent stopped.\n",
                agent.name
            ),
            "",
        ) {
            Ok(()) => true,
            Err(crate::ChildContextRecordError::InvalidStatus) => {
                return Err(StopError::new("active child cancellation lost lease"));
            }
            Err(_error) => return Err(StopError::new("cannot cancel owned child channel")),
        };
        if cancelled {
            let history = cancellation
                .history
                .as_ref()
                .ok_or_else(|| StopError::new("stop plan omitted cancellation history receipts"))?;
            let events = crate::owned_child_cancellation_events(
                &cancellation.parent_agent,
                &cancellation.child_agent,
            )
            .map_err(|_error| StopError::new("cannot build child cancellation events"))?;
            verify_read_file(&history.child_messages)?;
            replace_file(&history.child_state, "cancelled\n")?;
            append_history_file(
                &history.parent_events,
                &cancellation.parent_session,
                events.parent_event(),
            )?;
            append_history_file(
                &history.child_events,
                &cancellation.child_session_dir,
                events.child_event(),
            )?;
        }
    }
    replace_file(&agent.control.status, "dead\n")?;
    replace_file(&agent.control.pid, "\n")?;
    append_file(
        &agent.control.log,
        &serde_json::json!({
            "type": "agent.stop",
            "agent": agent.name,
            "status": "cancelled"
        })
        .to_string(),
    )?;
    if let Some(cleanup) = agent.cleanup {
        execute_temp_cleanup(cleanup)?;
    }
    let runtime = agent
        .runtime
        .as_ref()
        .ok_or_else(|| StopError::new("stop plan omitted runtime receipt"))?;
    stop_runtime(AgentRuntimeStop {
        terminal: &runtime.terminal,
        system: &runtime.system,
        terminal_live: runtime.terminal_live,
        system_live: runtime.system_live,
    })
    .map_err(|_error| StopError::new("agent stop runtime receipt conflict"))
}

/// Executes a fully preflighted concrete plan in descendant postorder.
pub fn execute_stop(plan: ConcreteStopPlan) -> Result<(), StopError> {
    preflight_concrete(&plan)?;
    for agent in plan.entries {
        stop_concrete_agent(agent)?;
    }
    Ok(())
}

fn order_agent(
    name: &str,
    candidates: &HashMap<String, Candidate>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
    ordered: &mut Vec<String>,
) -> Result<(), StopError> {
    if !visiting.insert(name.to_owned()) {
        return Err(StopError::new(format!(
            "agent stop ownership cycle at {name}"
        )));
    }
    if visited.contains(name) {
        return Err(StopError::new(format!(
            "duplicate agent in stop plan: {name}"
        )));
    }
    let mut children = candidates
        .iter()
        .filter(|&(_, candidate)| {
            candidate.lifecycle == crate::ChildLifecycle::Owned
                && candidate
                    .parent
                    .as_ref()
                    .map(|parent| parent.agent.as_str())
                    == Some(name)
        })
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    children.sort_unstable();
    for child in children {
        order_agent(child, candidates, visiting, visited, ordered)?;
    }
    visiting.remove(name);
    visited.insert(name.to_owned());
    ordered.push(name.to_owned());
    Ok(())
}

fn read_trimmed(path: &std::path::Path) -> Result<Option<String>, StopError> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value.trim().to_owned())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(StopError::new(format!(
            "cannot read {}: {error}",
            path.display()
        ))),
    }
}

fn parse_parent(value: &str) -> Result<StopParent, StopError> {
    let mut fields = value.split_whitespace();
    let agent = fields
        .next()
        .and_then(|field| field.strip_prefix("agent:"))
        .filter(|agent| crate::is_object_name(agent))
        .ok_or_else(|| StopError::new("invalid agent parent"))?;
    let mut session = None;
    for field in fields {
        let (kind, name) = field
            .split_once(':')
            .ok_or_else(|| StopError::new("invalid agent parent"))?;
        if !matches!(kind, "session" | "run") || !crate::is_object_name(name) {
            return Err(StopError::new("invalid agent parent"));
        }
        if kind == "session" && session.replace(name.to_owned()).is_some() {
            return Err(StopError::new("invalid agent parent"));
        }
    }
    Ok(StopParent {
        agent: agent.to_owned(),
        session,
    })
}

/// Failure while planning or executing a receipt-bound agent stop.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StopError {
    message: String,
}

impl StopError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for StopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for StopError {}

/// A validated, post-order stop plan.
///
/// The planner owns the receipt-bound entries.  Keeping the ordering executor
/// here lets the host CLI and privileged runtime use exactly the same lifecycle
/// state machine without sharing either process-global CLI state or protocol
/// framing.
#[derive(Debug)]
pub struct StopPlan<T> {
    entries: Vec<T>,
}

impl<T> StopPlan<T> {
    #[must_use]
    pub fn new(entries: Vec<T>) -> Self {
        Self { entries }
    }

    #[must_use]
    pub fn entries(&self) -> &[T] {
        &self.entries
    }
}

/// Receipt-bound operations required by the shared stop executor.
pub trait StopExecutor<T> {
    fn preflight(&mut self, plan: &StopPlan<T>) -> Result<(), StopError>;
    fn stop_entry(&mut self, entry: T) -> Result<(), StopError>;
}

/// Executes an already post-ordered stop plan.
///
/// Preflight covers the complete plan before the first mutation.  Entries are
/// then consumed in their planned order (owned descendants before their
/// parent); each adapter's `stop_entry` must retain terminal-first ordering and
/// leave the parent's system resource until its final operation.
pub fn execute<T>(plan: StopPlan<T>, executor: &mut impl StopExecutor<T>) -> Result<(), StopError> {
    executor.preflight(&plan)?;
    for entry in plan.entries {
        executor.stop_entry(entry)?;
    }
    Ok(())
}

/// Receipt-bound runtime resources selected for an agent stop.
#[derive(Clone, Copy, Debug)]
pub struct AgentRuntimeStop<'a> {
    pub terminal: &'a AgentLaunchReceipt,
    pub system: &'a SystemAgentSocketReceipt,
    pub terminal_live: bool,
    pub system_live: bool,
}

/// Stops receipt-bound runtime resources in terminal-first order.
///
/// Each stop operation verifies the receipt before removing its resource.
pub fn stop_runtime(stop: AgentRuntimeStop<'_>) -> Result<(), AgentLaunchError> {
    if stop.terminal_live {
        stop_launch(stop.terminal)?;
    }
    if stop.system_live {
        stop_system_agent_socket(stop.system)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(label: &str) -> PathBuf {
        use std::time::{SystemTime, UNIX_EPOCH};
        std::env::temp_dir().join(format!(
            "cortexfs-stop-{label}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ))
    }

    #[derive(Default)]
    struct RecordingExecutor {
        calls: Vec<String>,
        reject_preflight: bool,
    }

    impl StopExecutor<&'static str> for RecordingExecutor {
        fn preflight(&mut self, plan: &StopPlan<&'static str>) -> Result<(), StopError> {
            self.calls
                .push(format!("preflight:{}", plan.entries().len()));
            if self.reject_preflight {
                return Err(StopError::new("conflict"));
            }
            Ok(())
        }

        fn stop_entry(&mut self, entry: &'static str) -> Result<(), StopError> {
            self.calls.push(entry.to_owned());
            Ok(())
        }
    }

    #[test]
    fn stop_plan_preflights_before_consuming_postorder_entries() {
        let plan = StopPlan::new(vec!["grandchild", "child", "parent"]);
        let mut executor = RecordingExecutor::default();

        assert_eq!(execute(plan, &mut executor), Ok(()));
        assert_eq!(
            executor.calls,
            ["preflight:3", "grandchild", "child", "parent"]
        );
    }

    #[test]
    fn failed_preflight_performs_no_stop_mutation() {
        let plan = StopPlan::new(vec!["child", "parent"]);
        let mut executor = RecordingExecutor {
            reject_preflight: true,
            ..RecordingExecutor::default()
        };

        assert_eq!(
            execute(plan, &mut executor),
            Err(StopError::new("conflict"))
        );
        assert_eq!(executor.calls, ["preflight:2"]);
    }

    #[test]
    fn authorization_binds_owner_and_runtime_agent() {
        let context = StopContext {
            source: PathBuf::from("/source"),
            owner_uid: 1000,
            peer_uid: 1000,
            runtime_agent: "parent".to_owned(),
        };
        assert_eq!(authorize(&context, "parent"), Ok(()));

        let mut foreign = context.clone();
        foreign.peer_uid = 1001;
        assert_eq!(
            authorize(&foreign, "parent"),
            Err(StopError::new("agent stop peer denied"))
        );
        assert_eq!(
            authorize(&context, "child"),
            Err(StopError::new("agent stop runtime agent mismatch"))
        );
    }

    #[test]
    fn owned_and_missing_life_descendants_are_postordered_and_self_is_last()
    -> Result<(), Box<dyn std::error::Error>> {
        let source = test_root("order");
        let controls = source.join("agent");
        for (name, life, parent) in [
            ("parent", "owned", None),
            ("child", "owned", Some("agent:parent session:main")),
            ("grand", "owned", Some("agent:child session:main")),
            ("temporary", "temp", Some("agent:parent session:main")),
            ("detached", "owned", None),
        ] {
            let control = controls.join(format!("{name}.d"));
            fs::create_dir_all(&control)?;
            fs::write(control.join("life"), life)?;
            fs::write(control.join("parent"), parent.unwrap_or_default())?;
        }
        fs::remove_file(controls.join("child.d/life"))?;
        let context = StopContext {
            source: source.clone(),
            owner_uid: 1000,
            peer_uid: 1000,
            runtime_agent: "parent".to_owned(),
        };

        assert_eq!(
            ordered_owned_agents(&context, "parent")?,
            ["grand", "child", "parent"]
        );
        fs::remove_dir_all(source)?;
        Ok(())
    }

    #[test]
    fn late_receipt_conflict_prevents_all_earlier_mutation()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = test_root("late-conflict");
        let mut entries = Vec::new();
        for name in ["child", "parent"] {
            let control = root.join(format!("{name}.d"));
            fs::create_dir_all(&control)?;
            fs::write(control.join("status"), "alive\n")?;
            fs::write(control.join("pid"), "42\n")?;
            fs::write(control.join("log"), "")?;
            entries.push(PlannedStop {
                name: name.to_owned(),
                identity: None,
                stop_system_socket: false,
                terminal_units: Vec::new(),
                cancellations: Vec::new(),
                cleanup: None,
                runtime: None,
                control: bind_control(&control)?,
            });
        }
        let late_status = root.join("parent.d/status");
        fs::remove_file(&late_status)?;
        fs::write(&late_status, "replacement\n")?;

        assert!(execute_stop(StopPlan::new(entries)).is_err());
        assert_eq!(fs::read_to_string(root.join("child.d/status"))?, "alive\n");
        assert_eq!(fs::read_to_string(root.join("child.d/pid"))?, "42\n");
        assert_eq!(fs::read_to_string(root.join("child.d/log"))?, "");
        fs::remove_dir_all(root)?;
        Ok(())
    }

    #[test]
    fn cancellation_history_append_accepts_empty_migrated_marker_receipt()
    -> Result<(), Box<dyn std::error::Error>> {
        let session = test_root("columnar-history");
        fs::create_dir_all(&session)?;
        fs::write(session.join("messages.jsonl"), "")?;
        let marker = session.join("events.jsonl");
        let legacy = r#"{"type":"usage","run":"legacy","input_tokens":1,"output_tokens":0}"#;
        let migrated = r#"{"type":"usage","run":"migrated","input_tokens":1,"output_tokens":0}"#;
        fs::write(&marker, format!("{legacy}\n"))?;
        columnar::append(&session, columnar::Stream::Events, &[migrated])?;
        assert_eq!(fs::metadata(&marker)?.len(), 0);

        let receipt = bind_file(&marker, true)?;
        let events = crate::owned_child_cancellation_events("parent", "child")
            .map_err(|error| format!("{error:?}"))?;
        verify_file(&receipt, true)?;
        append_history_file(&receipt, &session, events.child_event())?;

        let projected = columnar::read_text(&session, columnar::Stream::Events, 4096)?;
        assert_eq!(
            projected,
            format!("{legacy}\n{migrated}\n{}\n", events.child_event())
        );
        assert!(crate::inspect_event_stream_jsonl(&projected).is_ok());
        assert_eq!(fs::metadata(&marker)?.len(), 0);
        fs::remove_dir_all(session)?;
        Ok(())
    }

    #[test]
    fn temp_cleanup_replacement_conflict_fails_closed() -> Result<(), Box<dyn std::error::Error>> {
        let root = test_root("cleanup");
        fs::create_dir_all(&root)?;
        let path = root.join("agent");
        fs::write(&path, "old")?;
        let metadata = fs::symlink_metadata(&path)?;
        let plan = TempCleanupPlan {
            entries: vec![TempCleanupEntry {
                path: path.clone(),
                directory: false,
                dev: metadata.dev(),
                ino: metadata.ino(),
                kind: metadata.mode() & libc::S_IFMT,
            }],
        };
        fs::remove_file(&path)?;
        fs::write(&path, "replacement")?;

        assert!(execute_temp_cleanup(plan).is_err());
        assert_eq!(fs::read_to_string(&path)?, "replacement");
        fs::remove_dir_all(root)?;
        Ok(())
    }
}
