//! Low-level user-systemd launch primitives shared by host CLI and agent tools.

use std::env;
use std::fs;
use std::io;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use crate::agent::create::{
    AgentCreatePaths, AgentRollbackConflict, AgentRollbackError, rollback_agent_files,
};
use crate::{
    AgentUnixIdentity, ChildContextRecordError, ChildHandoffReceipt, claim_child_handoff_active,
    rollback_child_handoff,
};

/// Persists receipt-bound cleanup evidence for one agent launch.
pub fn persist_agent_launch_meta(
    source: &Path,
    name: &str,
    terminal: &AgentLaunchReceipt,
    system: &SystemAgentSocketReceipt,
) -> Result<(), AgentLaunchError> {
    let control = source.join("agent").join(format!("{name}.d"));
    let control_meta =
        fs::symlink_metadata(&control).map_err(|_error| AgentLaunchError::CannotExecute)?;
    if !control_meta.is_dir() || control_meta.file_type().is_symlink() {
        return Err(AgentLaunchError::CannotExecute);
    }
    let session = terminal
        .socket
        .parent()
        .and_then(Path::file_name)
        .and_then(std::ffi::OsStr::to_str)
        .filter(|session| crate::is_object_name(session))
        .ok_or(AgentLaunchError::CannotExecute)?;
    let meta_path = control.join("meta.json");
    let (mut meta, create) = match crate::support::plain::read_small_text_file(&meta_path, 65_536) {
        Ok(content) => {
            let meta = serde_json::from_str::<serde_json::Value>(&content)
                .ok()
                .and_then(|value| value.as_object().cloned())
                .ok_or(AgentLaunchError::CannotExecute)?;
            (meta, false)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (serde_json::Map::new(), true),
        Err(_error) => return Err(AgentLaunchError::CannotExecute),
    };
    meta.insert(
        "runtime_receipt".to_owned(),
        serde_json::json!({
            "version": 1,
            "control": { "dev": control_meta.dev(), "ino": control_meta.ino() },
            "terminal": {
                "session": session,
                "unit": terminal.unit,
                "invocation": terminal.invocation,
                "pid": terminal.pid,
                "identity": {
                    "uid": terminal.identity.uid(),
                    "gid": terminal.identity.gid(),
                    "groups": terminal.identity.groups(),
                }
            },
            "system": {
                "unit": system.unit,
                "invocation": system.invocation,
                "owned_start": system.owned_start,
            }
        }),
    );
    let encoded = serde_json::to_string(&meta).map_err(|_error| AgentLaunchError::CannotExecute)?;
    let recorded = if create {
        crate::atomic_create_text_with_mode(&meta_path, &format!("{encoded}\n"), 0o644)
    } else {
        crate::atomic_replace_text_preserving_metadata(&meta_path, &format!("{encoded}\n"))
    };
    recorded.map_err(|_error| AgentLaunchError::CannotExecute)?;
    let rebound =
        fs::symlink_metadata(&control).map_err(|_error| AgentLaunchError::CannotExecute)?;
    if (rebound.dev(), rebound.ino()) != (control_meta.dev(), control_meta.ino()) {
        return Err(AgentLaunchError::StopConflict);
    }
    Ok(())
}

#[cfg(test)]
mod launch_meta_tests {
    use super::*;

    fn receipts() -> (AgentLaunchReceipt, SystemAgentSocketReceipt) {
        (
            AgentLaunchReceipt {
                unit: "terminal-unit".to_owned(),
                pid: 42,
                identity: AgentUnixIdentity::new(1000, 1000, [10, 20]),
                invocation: "terminal-invocation".to_owned(),
                socket: PathBuf::from("/run/user/1000/default/terminal.sock"),
            },
            SystemAgentSocketReceipt {
                unit: "system-unit".to_owned(),
                was_active: false,
                owned_start: true,
                invocation: "system-invocation".to_owned(),
            },
        )
    }

    #[test]
    fn missing_agent_meta_is_created_with_runtime_receipt() {
        let Ok(root) = tempfile::tempdir() else {
            return;
        };
        let control = root.path().join("agent/child.d");
        assert!(fs::create_dir_all(&control).is_ok());
        let (terminal, system) = receipts();

        assert_eq!(
            persist_agent_launch_meta(root.path(), "child", &terminal, &system),
            Ok(())
        );
        let meta_path = control.join("meta.json");
        let value = fs::read_to_string(&meta_path)
            .ok()
            .and_then(|content| serde_json::from_str::<serde_json::Value>(&content).ok());
        assert_eq!(
            value
                .as_ref()
                .and_then(|value| value.pointer("/runtime_receipt/terminal/pid"))
                .and_then(serde_json::Value::as_u64),
            Some(42)
        );
        assert!(matches!(
            fs::metadata(meta_path),
            Ok(metadata) if metadata.permissions().mode() & 0o7777 == 0o644
        ));
    }

    #[test]
    fn malformed_or_non_object_agent_meta_is_rejected() {
        let Ok(root) = tempfile::tempdir() else {
            return;
        };
        let control = root.path().join("agent/child.d");
        assert!(fs::create_dir_all(&control).is_ok());
        let meta_path = control.join("meta.json");
        let (terminal, system) = receipts();

        for content in ["{malformed\n", "[]\n"] {
            assert!(fs::write(&meta_path, content).is_ok());
            assert_eq!(
                persist_agent_launch_meta(root.path(), "child", &terminal, &system),
                Err(AgentLaunchError::CannotExecute)
            );
            assert_eq!(
                fs::read_to_string(&meta_path).ok().as_deref(),
                Some(content)
            );
        }
    }
}

pub(crate) fn ensure_terminal_runtime_dir(
    runtime: &Path,
    agent: &str,
    session: &str,
    identity: &AgentUnixIdentity,
) -> io::Result<fs::File> {
    let mut parent = crate::support::plain::open_plain_directory(runtime)?;
    let runtime_meta = parent.metadata()?;
    if runtime_meta.uid() != identity.uid() || runtime_meta.permissions().mode() & 0o7777 != 0o700 {
        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
    }
    for (name, mode) in [
        ("cortexfs", 0o755),
        ("terminal", 0o700),
        (agent, 0o700),
        (session, 0o700),
    ] {
        if !crate::is_object_name(name) {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let created = match nix::sys::stat::mkdirat(
            &parent,
            name,
            nix::sys::stat::Mode::from_bits_truncate(mode),
        ) {
            Ok(()) => true,
            Err(nix::errno::Errno::EEXIST) => false,
            Err(error) => return Err(io::Error::from(error)),
        };
        let fd = nix::fcntl::openat(
            &parent,
            name,
            nix::fcntl::OFlag::O_RDONLY
                | nix::fcntl::OFlag::O_DIRECTORY
                | nix::fcntl::OFlag::O_NOFOLLOW
                | nix::fcntl::OFlag::O_CLOEXEC,
            nix::sys::stat::Mode::empty(),
        )
        .map(fs::File::from)
        .map_err(io::Error::from)?;
        if created {
            nix::unistd::fchown(
                &fd,
                Some(nix::unistd::Uid::from_raw(identity.uid())),
                Some(nix::unistd::Gid::from_raw(identity.gid())),
            )
            .map_err(io::Error::from)?;
            nix::sys::stat::fchmod(&fd, nix::sys::stat::Mode::from_bits_truncate(mode))
                .map_err(io::Error::from)?;
        }
        let metadata = fd.metadata()?;
        if metadata.uid() != identity.uid()
            || metadata.gid() != identity.gid()
            || metadata.permissions().mode() & 0o7777 != mode
        {
            return Err(io::Error::from(io::ErrorKind::PermissionDenied));
        }
        parent = fd;
    }
    Ok(parent)
}

/// Validated target user-manager endpoint, independent of caller environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserManagerIdentity {
    uid: u32,
    gid: u32,
    groups: Vec<u32>,
    runtime: PathBuf,
    bus: PathBuf,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UserManagerCaller {
    Direct,
    Setpriv,
}

impl UserManagerIdentity {
    /// Validates `/run/user/<uid>` and its user-bus socket without following links.
    pub fn fresh(identity: &AgentUnixIdentity) -> io::Result<Self> {
        let runtime = PathBuf::from(format!("/run/user/{}", identity.uid()));
        let metadata = fs::symlink_metadata(&runtime)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != identity.uid()
            || metadata.permissions().mode() & 0o7777 != 0o700
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "invalid target user runtime directory",
            ));
        }
        let bus = runtime.join("bus");
        let bus_metadata = fs::symlink_metadata(&bus)?;
        if !bus_metadata.file_type().is_socket()
            || bus_metadata.file_type().is_symlink()
            || bus_metadata.uid() != identity.uid()
        {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "invalid target user bus socket",
            ));
        }
        Ok(Self {
            uid: identity.uid(),
            gid: identity.gid(),
            groups: identity.groups().to_vec(),
            runtime,
            bus,
        })
    }

    /// Builds an exact target-user command with a closed environment.
    #[expect(
        clippy::similar_names,
        reason = "uid and gid are paired identity fields"
    )]
    pub fn command(&self, program: &Path, args: &[String]) -> io::Result<Command> {
        let caller_uid = nix::unistd::geteuid().as_raw();
        let caller_gid = nix::unistd::getegid().as_raw();
        let groups = nix::unistd::getgroups()
            .map_err(io::Error::from)?
            .into_iter()
            .map(nix::unistd::Gid::as_raw)
            .collect::<Vec<_>>();
        match select_user_manager_caller(
            caller_uid,
            caller_gid,
            &groups,
            self.uid,
            self.gid,
            &self.groups,
        )? {
            UserManagerCaller::Direct => return Ok(self.direct_command(program, args)),
            UserManagerCaller::Setpriv => {}
        }
        Ok(self.setpriv_command(program, args))
    }

    fn direct_command(&self, program: &Path, args: &[String]) -> Command {
        let mut command = Command::new("/usr/bin/env");
        command.arg("-i");
        self.append_environment(&mut command);
        command.arg(program).args(args);
        command
    }

    fn setpriv_command(&self, program: &Path, args: &[String]) -> Command {
        let mut command = Command::new("/usr/bin/setpriv");
        command.args(["--reuid", self.uid.to_string().as_str()]);
        command.args(["--regid", self.gid.to_string().as_str()]);
        if self.groups.is_empty() {
            command.arg("--clear-groups");
        } else {
            command.arg("--groups").arg(
                self.groups
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        command.args([
            "--bounding-set=-all",
            "--no-new-privs",
            "/usr/bin/env",
            "-i",
        ]);
        self.append_environment(&mut command);
        command.arg(program);
        command.args(args);
        command
    }

    fn append_environment(&self, command: &mut Command) {
        command
            .arg("PATH=/usr/bin:/bin")
            .arg(format!("XDG_RUNTIME_DIR={}", self.runtime.display()))
            .arg(format!(
                "DBUS_SESSION_BUS_ADDRESS=unix:path={}",
                self.bus.display()
            ));
    }
}

#[expect(
    clippy::similar_names,
    reason = "uid and gid are paired identity fields"
)]
fn select_user_manager_caller(
    caller_uid: u32,
    caller_gid: u32,
    groups: &[u32],
    expected_uid: u32,
    expected_gid: u32,
    target_groups: &[u32],
) -> io::Result<UserManagerCaller> {
    let mut actual_groups = groups.to_vec();
    let mut expected_groups = target_groups.to_vec();
    actual_groups.sort_unstable();
    expected_groups.sort_unstable();
    if caller_uid == expected_uid && caller_gid == expected_gid && actual_groups == expected_groups
    {
        return Ok(UserManagerCaller::Direct);
    }
    if caller_uid == 0 {
        Ok(UserManagerCaller::Setpriv)
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "caller identity does not match target user",
        ))
    }
}

#[cfg(test)]
mod user_manager_tests {
    use super::{
        SystemAgentSocketReceipt, UserManagerCaller, compensate_unreceipted_system_start_with,
        ensure_terminal_runtime_dir, parse_unit_state, select_user_manager_caller,
        stop_system_agent_socket, system_agent_visible_socket, wait_system_agent_visible_socket,
    };
    use crate::AgentUnixIdentity;
    use std::fs;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn runtime_fixture(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("cfs-{name}-{}", std::process::id()));
        let _ignored = fs::remove_dir_all(&path);
        assert!(fs::create_dir_all(&path).is_ok());
        assert!(fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).is_ok());
        path
    }

    #[test]
    fn terminal_runtime_chain_is_target_owned_and_reusable() {
        let root = runtime_fixture("terminal-runtime-owner");
        let uid = nix::unistd::geteuid().as_raw();
        let gid = nix::unistd::getegid().as_raw();
        let identity = AgentUnixIdentity::new(uid, gid, []);
        let first = ensure_terminal_runtime_dir(&root, "worker", "default", &identity);
        assert!(first.is_ok());
        let metadata = first.and_then(|file| file.metadata());
        assert!(
            matches!(metadata, Ok(ref metadata) if metadata.uid() == uid && metadata.gid() == gid && metadata.permissions().mode() & 0o7777 == 0o700)
        );
        assert!(ensure_terminal_runtime_dir(&root, "worker", "default", &identity).is_ok());
    }

    #[test]
    fn terminal_runtime_chain_rejects_symlink_component() {
        let root = runtime_fixture("terminal-runtime-symlink");
        let outside = runtime_fixture("terminal-runtime-outside");
        assert!(std::os::unix::fs::symlink(&outside, root.join("cortexfs")).is_ok());
        let identity = AgentUnixIdentity::new(
            nix::unistd::geteuid().as_raw(),
            nix::unistd::getegid().as_raw(),
            [],
        );
        assert!(ensure_terminal_runtime_dir(&root, "worker", "default", &identity).is_err());
    }

    #[test]
    fn exact_self_identity_uses_direct_environment() {
        assert!(matches!(
            select_user_manager_caller(1000, 100, &[30, 20], 1000, 100, &[20, 30]),
            Ok(UserManagerCaller::Direct)
        ));
    }

    #[test]
    fn root_mismatch_uses_setpriv() {
        assert!(matches!(
            select_user_manager_caller(0, 0, &[0], 1000, 100, &[20, 30]),
            Ok(UserManagerCaller::Setpriv)
        ));
    }

    #[test]
    fn nonroot_wrong_self_groups_fails_closed() {
        assert!(matches!(
            select_user_manager_caller(1000, 100, &[20], 1000, 100, &[20, 30]),
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied
        ));
    }

    #[test]
    fn parses_generation_bound_active_unit_state() {
        let state =
            parse_unit_state("MainPID=123\nInvocationID=0123456789abcdef\nActiveState=active\n");
        assert!(matches!(
            state,
            Some(state)
                if state.pid == 123
                    && state.invocation == "0123456789abcdef"
                    && state.active == "active"
        ));
    }

    #[test]
    fn rejects_incomplete_unit_state() {
        assert!(parse_unit_state("MainPID=123\nActiveState=active\n").is_none());
    }

    #[test]
    fn preexisting_system_socket_receipt_is_never_stopped() {
        let receipt = SystemAgentSocketReceipt {
            unit: "must-not-exist.socket".to_owned(),
            was_active: true,
            owned_start: false,
            invocation: "preexisting".to_owned(),
        };
        assert_eq!(stop_system_agent_socket(&receipt), Ok(()));
    }

    #[test]
    fn system_socket_visible_path_matches_backing_agent_socket_abi() {
        assert_eq!(
            system_agent_visible_socket(std::path::Path::new("/storage/v1-root"), "child"),
            std::path::Path::new("/storage/v1-root/agent/child.sock")
        );
    }

    #[test]
    fn system_socket_visible_readiness_accepts_only_exact_runtime_symlink()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let runtime = root.path().join("runtime.sock");
        let visible = root.path().join("visible.sock");
        std::os::unix::fs::symlink(&runtime, &visible)?;
        assert_eq!(
            wait_system_agent_visible_socket(&visible, &runtime, 1, std::time::Duration::ZERO),
            Ok(())
        );
        assert!(
            wait_system_agent_visible_socket(
                &visible,
                &root.path().join("other.sock"),
                1,
                std::time::Duration::ZERO
            )
            .is_err()
        );
        Ok(())
    }

    #[test]
    fn owned_start_without_receipt_is_stopped_and_bounded() {
        let mut states = vec![Some("inactive".to_owned()), Some("active".to_owned())];
        let result = compensate_unreceipted_system_start_with(
            || Ok(true),
            || states.pop().flatten(),
            2,
            || {},
        );
        assert_eq!(result, Ok(()));
        assert_eq!(
            compensate_unreceipted_system_start_with(
                || Ok(false),
                || Some("active".to_owned()),
                1,
                || {}
            ),
            Err(super::AgentLaunchError::StopConflict)
        );
        assert_eq!(
            compensate_unreceipted_system_start_with(
                || Ok(true),
                || Some("active".to_owned()),
                2,
                || {}
            ),
            Err(super::AgentLaunchError::StopConflict)
        );
    }
}

/// Builds a command for the exact validated target user manager.
pub fn user_manager_command(
    identity: &AgentUnixIdentity,
    program: &Path,
    args: &[String],
) -> io::Result<Command> {
    UserManagerIdentity::fresh(identity)?.command(program, args)
}

/// Fully constructed supervisor command, independent of CLI parsing or output.
#[derive(Debug, Eq, PartialEq)]
pub struct AgentLaunchCommand {
    /// Absolute supervisor program.
    pub program: String,
    /// Ordered supervisor arguments.
    pub args: Vec<String>,
}

/// Receipt binding a successful launch to its user-systemd unit and live PID.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentLaunchReceipt {
    /// Transient unit name without the `.service` suffix.
    pub unit: String,
    /// Non-zero main process id observed after readiness.
    pub pid: u32,
    /// Exact target Unix identity used to contact the user manager.
    pub identity: AgentUnixIdentity,
    /// Systemd invocation id binding this receipt to one service generation.
    pub invocation: String,
    /// Runtime terminal socket used by this launch.
    pub socket: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemAgentSocketReceipt {
    pub unit: String,
    pub was_active: bool,
    pub owned_start: bool,
    pub invocation: String,
}

#[must_use]
pub fn system_agent_socket_unit(agent: &str) -> String {
    format!("cortexfs-agent@{agent}.socket")
}

#[must_use]
pub fn system_agent_runtime_socket(agent: &str) -> PathBuf {
    Path::new("/run/cortexfs/agent").join(format!("{agent}.sock"))
}

#[must_use]
pub fn system_agent_visible_socket(source: &Path, agent: &str) -> PathBuf {
    source.join("agent").join(format!("{agent}.sock"))
}

pub fn ensure_system_agent_socket(
    agent: &str,
    visible: &Path,
) -> Result<SystemAgentSocketReceipt, AgentLaunchError> {
    let unit = system_agent_socket_unit(agent);
    let before = system_unit_state(&unit);
    let was_active = before
        .as_ref()
        .is_some_and(|state| state.active == "active");
    if !was_active {
        let status = systemctl_system(&["start", &unit])
            .status()
            .map_err(|_error| AgentLaunchError::CannotExecute)?;
        if !status.success() {
            return Err(AgentLaunchError::Rejected);
        }
    }
    let state = match system_unit_state(&unit) {
        Some(state) => state,
        None if !was_active => {
            return Err(compensate_unreceipted_system_start(&unit)
                .err()
                .unwrap_or(AgentLaunchError::NotReady));
        }
        None => return Err(AgentLaunchError::NotReady),
    };
    if state.active != "active" || state.invocation.is_empty() {
        if !was_active {
            return Err(compensate_unreceipted_system_start(&unit)
                .err()
                .unwrap_or(AgentLaunchError::NotReady));
        }
        return Err(AgentLaunchError::NotReady);
    }
    let receipt = SystemAgentSocketReceipt {
        unit,
        was_active,
        owned_start: !was_active,
        invocation: state.invocation,
    };
    let runtime = system_agent_runtime_socket(agent);
    if wait_socket(&runtime, 50, Duration::from_millis(100)).is_err()
        || wait_system_agent_visible_socket(visible, &runtime, 50, Duration::from_millis(100))
            .is_err()
    {
        return Err(stop_system_agent_socket(&receipt)
            .err()
            .unwrap_or(AgentLaunchError::NotReady));
    }
    Ok(receipt)
}

fn wait_system_agent_visible_socket(
    visible: &Path,
    runtime: &Path,
    attempts: usize,
    delay: Duration,
) -> Result<(), AgentLaunchError> {
    for _ in 0..attempts {
        if fs::symlink_metadata(visible).is_ok_and(|metadata| metadata.file_type().is_symlink())
            && fs::read_link(visible).is_ok_and(|target| target == runtime)
        {
            return Ok(());
        }
        thread::sleep(delay);
    }
    Err(AgentLaunchError::NotReady)
}

fn compensate_unreceipted_system_start(unit: &str) -> Result<(), AgentLaunchError> {
    compensate_unreceipted_system_start_with(
        || {
            systemctl_system(&["stop", unit])
                .status()
                .map(|status| status.success())
        },
        || system_unit_state(unit).map(|state| state.active),
        50,
        || thread::sleep(Duration::from_millis(20)),
    )
}

fn compensate_unreceipted_system_start_with(
    mut stop: impl FnMut() -> io::Result<bool>,
    mut active: impl FnMut() -> Option<String>,
    attempts: usize,
    mut wait: impl FnMut(),
) -> Result<(), AgentLaunchError> {
    if !stop().is_ok_and(|success| success) {
        return Err(AgentLaunchError::StopConflict);
    }
    for _ in 0..attempts {
        match active().as_deref() {
            None | Some("inactive" | "failed") => return Ok(()),
            Some(_) => wait(),
        }
    }
    Err(AgentLaunchError::StopConflict)
}

pub fn stop_system_agent_socket(
    receipt: &SystemAgentSocketReceipt,
) -> Result<(), AgentLaunchError> {
    if !receipt.owned_start {
        return Ok(());
    }
    let Some(before) = system_unit_state(&receipt.unit) else {
        return Ok(());
    };
    if matches!(before.active.as_str(), "inactive" | "failed") {
        return Ok(());
    }
    if before.invocation != receipt.invocation {
        return Err(AgentLaunchError::StopConflict);
    }
    let status = systemctl_system(&["stop", &receipt.unit])
        .status()
        .map_err(|_error| AgentLaunchError::StopConflict)?;
    if !status.success() {
        return Err(AgentLaunchError::StopConflict);
    }
    for _ in 0..50 {
        match system_unit_state(&receipt.unit) {
            None => return Ok(()),
            Some(state) if state.active == "inactive" || state.active == "failed" => return Ok(()),
            Some(state) if state.invocation != receipt.invocation => {
                return Err(AgentLaunchError::StopConflict);
            }
            Some(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
    Err(AgentLaunchError::StopConflict)
}

/// Preflights an exact system socket generation without mutating it.
pub fn verify_system_agent_socket(
    receipt: &SystemAgentSocketReceipt,
) -> Result<bool, AgentLaunchError> {
    if !receipt.owned_start {
        return Ok(false);
    }
    let Some(state) = system_unit_state(&receipt.unit) else {
        return Ok(false);
    };
    if state.invocation != receipt.invocation {
        return Err(AgentLaunchError::StopConflict);
    }
    Ok(matches!(state.active.as_str(), "active" | "activating"))
}

/// Stable low-level launch failure.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentLaunchError {
    /// The supervisor process could not be executed.
    CannotExecute,
    /// The supervisor rejected the transient unit.
    Rejected,
    /// The service failed to become ready with a live main process.
    NotReady,
    /// Compensating stop failed to prove the launched unit stopped.
    StopConflict,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct UnitState {
    pid: u32,
    invocation: String,
    active: String,
}

struct SystemUnitState {
    invocation: String,
    active: String,
}

/// Verifies that a newly launched service is active and binds a receipt to its generation.
pub fn launch_receipt(
    identity: &AgentUnixIdentity,
    unit: &str,
    socket: PathBuf,
) -> Result<AgentLaunchReceipt, AgentLaunchError> {
    let state = unit_state(identity, unit).ok_or(AgentLaunchError::NotReady)?;
    if state.pid == 0 || state.invocation.is_empty() || state.active != "active" {
        return Err(AgentLaunchError::NotReady);
    }
    Ok(AgentLaunchReceipt {
        unit: unit.to_owned(),
        pid: state.pid,
        identity: identity.clone(),
        invocation: state.invocation,
        socket,
    })
}

/// Stops only the exact service generation represented by `receipt`.
pub fn stop_launch(receipt: &AgentLaunchReceipt) -> Result<(), AgentLaunchError> {
    let Some(before) = unit_state(&receipt.identity, &receipt.unit) else {
        return Ok(());
    };
    if matches!(before.active.as_str(), "inactive" | "failed") {
        return Ok(());
    }
    if before.invocation != receipt.invocation {
        return Err(AgentLaunchError::StopConflict);
    }
    let service = format!("{}.service", receipt.unit);
    let status = systemctl_user(&receipt.identity, &["stop", &service])
        .map_err(|_error| AgentLaunchError::StopConflict)?
        .status()
        .map_err(|_error| AgentLaunchError::StopConflict)?;
    if !status.success() {
        return Err(AgentLaunchError::StopConflict);
    }
    for _ in 0..50 {
        match unit_state(&receipt.identity, &receipt.unit) {
            None => return Ok(()),
            Some(state) if state.active == "inactive" || state.active == "failed" => return Ok(()),
            Some(state) if state.invocation != receipt.invocation => {
                return Err(AgentLaunchError::StopConflict);
            }
            Some(_) => thread::sleep(Duration::from_millis(20)),
        }
    }
    Err(AgentLaunchError::StopConflict)
}

/// Preflights an exact user service generation without mutating it.
pub fn verify_launch(receipt: &AgentLaunchReceipt) -> Result<bool, AgentLaunchError> {
    verify_launch_state(receipt, unit_state(&receipt.identity, &receipt.unit))
}

fn verify_launch_state(
    receipt: &AgentLaunchReceipt,
    state: Option<UnitState>,
) -> Result<bool, AgentLaunchError> {
    let Some(state) = state else {
        return Ok(false);
    };
    if state.invocation != receipt.invocation {
        return Err(AgentLaunchError::StopConflict);
    }
    let live = matches!(state.active.as_str(), "active" | "activating");
    if live && state.pid != receipt.pid {
        return Err(AgentLaunchError::StopConflict);
    }
    Ok(live)
}

#[cfg(test)]
mod receipt {
    use super::*;

    fn sample() -> AgentLaunchReceipt {
        AgentLaunchReceipt {
            unit: "unit".to_owned(),
            pid: 42,
            identity: AgentUnixIdentity::new(1000, 1000, []),
            invocation: "expected".to_owned(),
            socket: PathBuf::new(),
        }
    }

    #[test]
    fn launch_receipt_rejects_invocation_and_live_pid_reuse_without_mutation() {
        let receipt = sample();
        assert_eq!(verify_launch_state(&receipt, None), Ok(false));
        assert_eq!(
            verify_launch_state(
                &receipt,
                Some(UnitState {
                    pid: 42,
                    invocation: "replaced".to_owned(),
                    active: "active".to_owned(),
                })
            ),
            Err(AgentLaunchError::StopConflict)
        );
        assert_eq!(
            verify_launch_state(
                &receipt,
                Some(UnitState {
                    pid: 99,
                    invocation: "expected".to_owned(),
                    active: "active".to_owned(),
                })
            ),
            Err(AgentLaunchError::StopConflict)
        );
        assert_eq!(
            verify_launch_state(
                &receipt,
                Some(UnitState {
                    pid: 0,
                    invocation: "expected".to_owned(),
                    active: "inactive".to_owned(),
                })
            ),
            Ok(false)
        );
    }
}

fn unit_state(identity: &AgentUnixIdentity, unit: &str) -> Option<UnitState> {
    let service = format!("{unit}.service");
    let output = systemctl_user(
        identity,
        &[
            "show",
            "--property=MainPID",
            "--property=InvocationID",
            "--property=ActiveState",
            &service,
        ],
    )
    .ok()?
    .output()
    .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_unit_state(&String::from_utf8_lossy(&output.stdout))
}

fn parse_unit_state(output: &str) -> Option<UnitState> {
    let mut pid = None;
    let mut invocation = None;
    let mut active = None;
    for line in output.lines() {
        let (key, value) = line.split_once('=')?;
        match key {
            "MainPID" => pid = value.parse().ok(),
            "InvocationID" => invocation = Some(value.to_owned()),
            "ActiveState" => active = Some(value.to_owned()),
            _ => {}
        }
    }
    Some(UnitState {
        pid: pid?,
        invocation: invocation?,
        active: active?,
    })
}

/// Receipt for an agent-visible chat socket alias.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentChatAliasState {
    /// The alias already existed with the requested target.
    ExistingSameTarget,
    /// This transaction created the alias.
    Created,
    /// This transaction replaced the ordinary socket placeholder.
    ReplacedPlaceholder { mode: u32, uid: u32, gid: u32 },
}

/// Publishes an agent chat alias without following either parent or leaf links.
pub fn ensure_agent_chat_socket(visible: &Path, runtime: &Path) -> io::Result<AgentChatAliasState> {
    use nix::fcntl::AtFlags;
    use nix::sys::stat::{SFlag, fstatat};
    let parent = visible.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let dir = crate::support::plain::open_plain_directory(parent)?;
    let name = visible
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(io::ErrorKind::InvalidInput)?;
    let receipt = match fstatat(&dir, name, AtFlags::AT_SYMLINK_NOFOLLOW) {
        Ok(stat) if SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFLNK) => {
            let target = nix::fcntl::readlinkat(&dir, name).map_err(io::Error::from)?;
            if Path::new(&target) == runtime {
                return Ok(AgentChatAliasState::ExistingSameTarget);
            }
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "chat alias target differs",
            ));
        }
        Ok(stat) if SFlag::from_bits_truncate(stat.st_mode).contains(SFlag::S_IFSOCK) => {
            let current =
                fstatat(&dir, name, AtFlags::AT_SYMLINK_NOFOLLOW).map_err(io::Error::from)?;
            if (current.st_dev, current.st_ino) != (stat.st_dev, stat.st_ino)
                || !SFlag::from_bits_truncate(current.st_mode).contains(SFlag::S_IFSOCK)
            {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "chat placeholder changed",
                ));
            }
            nix::unistd::unlinkat(&dir, name, nix::unistd::UnlinkatFlags::NoRemoveDir)
                .map_err(io::Error::from)?;
            AgentChatAliasState::ReplacedPlaceholder {
                mode: stat.st_mode & 0o7777,
                uid: stat.st_uid,
                gid: stat.st_gid,
            }
        }
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "chat alias is not a socket",
            ));
        }
        Err(nix::errno::Errno::ENOENT) => AgentChatAliasState::Created,
        Err(error) => return Err(io::Error::from(error)),
    };
    if let Err(error) = nix::unistd::symlinkat(runtime, &dir, name) {
        if let AgentChatAliasState::ReplacedPlaceholder { mode, uid, gid } = receipt {
            drop(restore_chat_placeholder(visible, mode, uid, gid));
        }
        return Err(io::Error::from(error));
    }
    dir.sync_all()?;
    Ok(receipt)
}

/// Rolls back only the alias still owned by `receipt`.
pub fn rollback_agent_chat_alias(
    visible: &Path,
    runtime: &Path,
    receipt: &AgentChatAliasState,
) -> io::Result<()> {
    if matches!(receipt, AgentChatAliasState::ExistingSameTarget) {
        return Ok(());
    }
    let parent = visible.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let dir = crate::support::plain::open_plain_directory(parent)?;
    let name = visible
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(io::ErrorKind::InvalidInput)?;
    let target = nix::fcntl::readlinkat(&dir, name).map_err(io::Error::from)?;
    if Path::new(&target) != runtime {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "chat alias ownership changed",
        ));
    }
    nix::unistd::unlinkat(&dir, name, nix::unistd::UnlinkatFlags::NoRemoveDir)
        .map_err(io::Error::from)?;
    dir.sync_all()?;
    if let AgentChatAliasState::ReplacedPlaceholder { mode, uid, gid } = *receipt {
        restore_chat_placeholder(visible, mode, uid, gid)?;
    }
    Ok(())
}

fn restore_chat_placeholder(path: &Path, mode: u32, uid: u32, gid: u32) -> io::Result<()> {
    crate::support::plain::ensure_socket_placeholder(path, mode)?;
    let parent = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    let dir = crate::support::plain::open_plain_directory(parent)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(io::ErrorKind::InvalidInput)?;
    let original = placeholder_identity(&dir, name)?;
    nix::unistd::fchownat(
        &dir,
        name,
        Some(nix::unistd::Uid::from_raw(uid)),
        Some(nix::unistd::Gid::from_raw(gid)),
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(io::Error::from)?;
    require_placeholder_identity(&dir, name, original)?;
    nix::sys::stat::fchmodat(
        &dir,
        name,
        nix::sys::stat::Mode::from_bits_truncate(mode & 0o7777),
        nix::sys::stat::FchmodatFlags::NoFollowSymlink,
    )
    .map_err(io::Error::from)?;
    require_placeholder_identity(&dir, name, original)?;
    dir.sync_all()
}

fn placeholder_identity(dir: &fs::File, name: &str) -> io::Result<(u64, u64)> {
    let stat = nix::sys::stat::fstatat(dir, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .map_err(io::Error::from)?;
    if !nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode)
        .contains(nix::sys::stat::SFlag::S_IFSOCK)
    {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "restored chat placeholder changed",
        ));
    }
    Ok((stat.st_dev, stat.st_ino))
}

fn require_placeholder_identity(
    dir: &fs::File,
    name: &str,
    expected: (u64, u64),
) -> io::Result<()> {
    if placeholder_identity(dir, name)? == expected {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "restored chat placeholder identity changed",
        ))
    }
}

/// Waits for a plain Unix socket created by a launched service.
///
/// The caller supplies the bounded retry policy so host and tool launches use
/// the same readiness predicate without embedding CLI diagnostics here.
pub fn wait_socket(path: &Path, attempts: usize, delay: Duration) -> Result<(), AgentLaunchError> {
    for _ in 0..attempts {
        if fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_socket()) {
            return Ok(());
        }
        thread::sleep(delay);
    }
    Err(AgentLaunchError::NotReady)
}

/// Failure from the create → pending → launch → active transaction.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentLaunchTransactionError {
    /// The dedicated runtime did not launch or become ready.
    Launch(AgentLaunchError),
    /// The pending channel could not be claimed active.
    Claim(ChildContextRecordError),
    /// A compensating runtime stop could not be proved.
    StopConflict,
    /// The handoff inode could not be safely rolled back.
    HandoffConflict,
    /// The agent inode could not be safely rolled back.
    AgentConflict(AgentRollbackConflict),
}

/// Completes an already-created agent and pending handoff as one compensated launch.
///
/// On every failure the ordering is fixed: stop a returned launch receipt, roll
/// back the handoff receipt, then roll back the agent receipt.
#[expect(
    dead_code,
    reason = "wired when the remaining terminal launch closure is migrated"
)]
pub(crate) fn complete_child_launch(
    agent: AgentCreatePaths,
    handoff: &ChildHandoffReceipt,
    child_agent: &str,
    child_session: &str,
    launch: impl FnOnce() -> Result<AgentLaunchReceipt, AgentLaunchError>,
    stop: impl FnOnce(&AgentLaunchReceipt) -> Result<(), AgentLaunchError>,
) -> Result<AgentLaunchReceipt, AgentLaunchTransactionError> {
    let launched = match launch() {
        Ok(receipt) => receipt,
        Err(error) => {
            rollback_created(agent, handoff)?;
            return Err(AgentLaunchTransactionError::Launch(error));
        }
    };
    if let Err(error) = claim_child_handoff_active(handoff, child_agent, child_session) {
        if stop(&launched).is_err() {
            return Err(AgentLaunchTransactionError::StopConflict);
        }
        rollback_created(agent, handoff)?;
        return Err(AgentLaunchTransactionError::Claim(error));
    }
    Ok(launched)
}

#[expect(dead_code, reason = "used by the withheld launch coordinator")]
fn rollback_created(
    agent: AgentCreatePaths,
    handoff: &ChildHandoffReceipt,
) -> Result<(), AgentLaunchTransactionError> {
    if rollback_child_handoff(handoff).is_err() {
        return Err(AgentLaunchTransactionError::HandoffConflict);
    }
    match rollback_agent_files(agent) {
        Ok(()) => Ok(()),
        Err(AgentRollbackError::Conflict(conflict)) => {
            Err(AgentLaunchTransactionError::AgentConflict(conflict))
        }
    }
}

/// Validated values needed to launch one agent runtime independently of CLI syntax.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentLaunchRequest {
    /// Agent object name.
    pub agent: String,
    /// Dedicated durable session name.
    pub session: String,
    /// Host source tree consumed by the runtime.
    pub source: PathBuf,
    /// Sandbox working directory.
    pub cwd: String,
    /// Extra host mounts requested by the caller, in declaration order.
    pub mounts: Vec<AgentLaunchMount>,
    /// Whether the first writable workspace mount uses CLI workspace semantics.
    pub default_workspace: bool,
}

/// One validated caller-supplied sandbox mount.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentLaunchMount {
    /// Absolute host source.
    pub source: String,
    /// Absolute sandbox target.
    pub target: String,
    /// `ro` or `rw`.
    pub mode: String,
}

/// Constructs the terminal supervisor command shared by host and lifecycle launches.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "ordered supervisor argv is one auditable ABI command"
)]
pub fn terminal_command(
    request: &AgentLaunchRequest,
    view: &crate::AgentRuntimeView,
    socket: &Path,
    unit: &str,
) -> AgentLaunchCommand {
    let mut command = AgentLaunchCommand {
        program: "/usr/bin/systemd-run".to_owned(),
        args: vec![
            "--user".to_owned(),
            "--unit".to_owned(),
            unit.to_owned(),
            "--property".to_owned(),
            "Restart=always".to_owned(),
            "--property".to_owned(),
            "RestartSec=250ms".to_owned(),
            "/usr/bin/env".to_owned(),
            "-i".to_owned(),
            "PATH=/usr/bin:/bin".to_owned(),
            "/usr/bin/bwrap".to_owned(),
            "--clearenv".to_owned(),
        ],
    };
    for (key, value) in terminal_env(view) {
        command.args.extend(["--setenv".to_owned(), key, value]);
    }
    if let Some(workspace) = request
        .mounts
        .iter()
        .rev()
        .find(|mount| mount.target == "/workspace" && mount.mode == "rw")
    {
        command.args.extend([
            "--setenv".to_owned(),
            "CTX_WORKSPACE".to_owned(),
            workspace.source.clone(),
        ]);
    }
    command.args.extend([
        "--die-with-parent".to_owned(),
        "--unshare-pid".to_owned(),
        "--unshare-net".to_owned(),
        "--proc".to_owned(),
        "/proc".to_owned(),
        "--dev".to_owned(),
        "/dev".to_owned(),
        "--tmpfs".to_owned(),
        "/tmp".to_owned(),
        "--dir".to_owned(),
        "/run".to_owned(),
        "--dir".to_owned(),
        "/home".to_owned(),
        "--ro-bind".to_owned(),
        "/usr".to_owned(),
        "/usr".to_owned(),
        "--ro-bind".to_owned(),
        "/etc".to_owned(),
        "/etc".to_owned(),
        "--tmpfs".to_owned(),
        "/etc/profile.d".to_owned(),
        "--symlink".to_owned(),
        "usr/bin".to_owned(),
        "/bin".to_owned(),
        "--symlink".to_owned(),
        "usr/lib".to_owned(),
        "/lib".to_owned(),
        "--symlink".to_owned(),
        "usr/lib".to_owned(),
        "/lib64".to_owned(),
    ]);
    if let Some(runtime_dir) = socket.parent() {
        command.args.extend([
            "--bind".to_owned(),
            runtime_dir.display().to_string(),
            runtime_dir.display().to_string(),
        ]);
    }
    for mount in view.mount_table().entries() {
        if request.default_workspace && mount.target() == "/workspace/.git" {
            continue;
        }
        if mount.target() == view.home() || mount.target() == "/home/agent" {
            continue;
        }
        command.args.push(
            if mount.mode() == crate::MountMode::ReadOnly {
                "--ro-bind"
            } else {
                "--bind"
            }
            .to_owned(),
        );
        command
            .args
            .push(host_mount_source(&request.source, mount.source()));
        command.args.push(mount.target().to_owned());
    }
    for mount in &request.mounts {
        if mount.target == "/home/agent" {
            continue;
        }
        command.args.push(
            if mount.mode == "ro" {
                "--ro-bind"
            } else {
                "--bind"
            }
            .to_owned(),
        );
        command.args.push(mount.source.clone());
        command.args.push(mount.target.clone());
    }
    command.args.extend([
        "--bind".to_owned(),
        view.home().display().to_string(),
        "/home/agent".to_owned(),
    ]);
    let startup = socket
        .parent()
        .map(|parent| parent.join(".empty-shell-startup"));
    if let Some(startup) = startup {
        command.args.extend([
            "--ro-bind".to_owned(),
            startup.display().to_string(),
            "/etc/profile".to_owned(),
            "--ro-bind".to_owned(),
            startup.display().to_string(),
            "/etc/bash.bashrc".to_owned(),
        ]);
    }
    command.args.extend([
        "--chdir".to_owned(),
        request.cwd.clone(),
        "/usr/bin/ctxterm".to_owned(),
        "--listen".to_owned(),
        socket.display().to_string(),
        "--no-stdio".to_owned(),
        "--".to_owned(),
        "/ctx/bin/tsh".to_owned(),
    ]);
    command
}

fn terminal_env(view: &crate::AgentRuntimeView) -> Vec<(String, String)> {
    let groups = view
        .identity()
        .groups()
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(" ");
    let owner = view
        .ctx_home()
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("1000");
    let life = match view.lifecycle() {
        crate::ChildLifecycle::Owned => "owned",
        crate::ChildLifecycle::Temp => "temp",
    };
    let mut env = vec![
        ("CTX_ROOT".to_owned(), "/ctx".to_owned()),
        (
            "CTX_PROVIDER_CONFIG_DIR".to_owned(),
            "/ctx/shared/providers.d".to_owned(),
        ),
        ("CTX_HOME".to_owned(), format!("/ctx/home/{owner}")),
        ("CTX_AGENT".to_owned(), view.agent_name().to_owned()),
        ("CTX_AGENT_ROLE".to_owned(), "agent".to_owned()),
        ("CTX_AGENT_MODEL".to_owned(), view.model().to_owned()),
        ("CTX_AGENT_LIFE".to_owned(), life.to_owned()),
        (
            "CTX_AGENT_ROOT_PATH".to_owned(),
            view.root().display().to_string(),
        ),
        ("CTX_AGENT_CWD".to_owned(), view.cwd().display().to_string()),
        (
            "CTX_AGENT_SUBJECT".to_owned(),
            view.policy_subject().to_owned(),
        ),
        (
            "CTX_AGENT_UID".to_owned(),
            view.identity().uid().to_string(),
        ),
        (
            "CTX_AGENT_GID".to_owned(),
            view.identity().gid().to_string(),
        ),
        ("CTX_AGENT_GROUPS".to_owned(), groups),
        ("HOME".to_owned(), "/home/agent".to_owned()),
        ("USER".to_owned(), view.agent_name().to_owned()),
        ("LOGNAME".to_owned(), view.agent_name().to_owned()),
        ("SHELL".to_owned(), "/usr/bin/bash".to_owned()),
        ("TERM".to_owned(), "xterm-256color".to_owned()),
        ("LANG".to_owned(), "C.UTF-8".to_owned()),
        ("GIT_OPTIONAL_LOCKS".to_owned(), "0".to_owned()),
        ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
    ];
    for pair in view.env() {
        if !env.iter().any(|entry| entry.0 == pair.0) {
            env.push(pair.clone());
        }
    }
    env
}

fn host_mount_source(root: &Path, source: &str) -> String {
    let source = Path::new(source);
    source.strip_prefix("/ctx").map_or_else(
        |_| source.display().to_string(),
        |relative| root.join(relative).display().to_string(),
    )
}

/// Constructs the socket-activated model runtime command for an agent.
#[must_use]
pub fn chat_socket_command(
    request: &AgentLaunchRequest,
    socket: &Path,
    unit: &str,
    runtime_program: &Path,
) -> AgentLaunchCommand {
    AgentLaunchCommand {
        program: "/usr/bin/systemd-run".to_owned(),
        args: vec![
            "--user".to_owned(),
            "--unit".to_owned(),
            unit.to_owned(),
            "--collect".to_owned(),
            "--socket-property".to_owned(),
            format!("ListenStream={}", socket.display()),
            "--socket-property".to_owned(),
            "SocketMode=0666".to_owned(),
            runtime_program.display().to_string(),
            "--source".to_owned(),
            request.source.display().to_string(),
            "--agent".to_owned(),
            request.agent.clone(),
        ],
    }
}

/// Builds a process from a typed launch command with only user-systemd client environment.
pub fn launch_process_for(
    identity: &AgentUnixIdentity,
    command: &AgentLaunchCommand,
) -> io::Result<Command> {
    user_manager_command(identity, Path::new(&command.program), &command.args)
}

/// Retains only environment required to contact the current user's systemd manager.
pub fn set_user_systemd_client_env(command: &mut Command) {
    command.env_clear().env("PATH", "/usr/bin:/bin");
    for key in ["XDG_RUNTIME_DIR", "DBUS_SESSION_BUS_ADDRESS"] {
        if let Some(value) = env::var_os(key) {
            command.env(key, value);
        }
    }
}

/// Extracts the invocation id printed by `systemd-run`.
#[must_use]
pub fn invocation_id(output: &Output) -> Option<String> {
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    text.lines().find_map(|line| {
        line.rsplit_once("invocation ID: ")
            .map(|(_prefix, value)| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    })
}

/// Reads a non-zero main pid from a user service.
#[must_use]
pub fn unit_main_pid_for(identity: &AgentUnixIdentity, unit: &str) -> Option<u32> {
    let service = format!("{unit}.service");
    let output = systemctl_user(
        identity,
        &["show", "--property", "MainPID", "--value", &service],
    )
    .ok()?
    .output()
    .ok()?;
    output
        .status
        .success()
        .then(|| parse_main_pid(&String::from_utf8_lossy(&output.stdout)))
        .flatten()
}

/// Parses systemd's decimal `MainPID` value, rejecting zero.
#[must_use]
pub fn parse_main_pid(output: &str) -> Option<u32> {
    let pid = output.trim().parse::<u32>().ok()?;
    (pid != 0).then_some(pid)
}

/// Stops and clears one transient user service.
pub fn reset_unit_for(identity: &AgentUnixIdentity, unit: &str) {
    let service = format!("{unit}.service");
    for verb in ["stop", "reset-failed"] {
        let Ok(mut command) = systemctl_user(identity, &[verb, &service]) else {
            continue;
        };
        let _ignored = command.stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

/// Stops and clears both units created by a socket-activated transient service.
pub fn reset_socket_unit_for(identity: &AgentUnixIdentity, unit: &str) {
    for suffix in ["service", "socket"] {
        let target = format!("{unit}.{suffix}");
        for verb in ["stop", "reset-failed"] {
            let Ok(mut command) = systemctl_user(identity, &[verb, target.as_str()]) else {
                continue;
            };
            let _ignored = command.stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
    }
}

fn systemctl_user(identity: &AgentUnixIdentity, arguments: &[&str]) -> io::Result<Command> {
    let mut argv = vec!["--user".to_owned()];
    argv.extend(arguments.iter().map(ToString::to_string));
    user_manager_command(identity, Path::new("/usr/bin/systemctl"), &argv)
}

fn systemctl_system(arguments: &[&str]) -> Command {
    let mut command = Command::new("/usr/bin/systemctl");
    command
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .arg("--no-ask-password")
        .args(arguments);
    command
}

fn system_unit_state(unit: &str) -> Option<SystemUnitState> {
    let output = systemctl_system(&[
        "show",
        "--property=InvocationID",
        "--property=ActiveState",
        unit,
    ])
    .output()
    .ok()?;
    if !output.status.success() {
        return None;
    }
    let mut invocation = None;
    let mut active = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let (key, value) = line.split_once('=')?;
        match key {
            "InvocationID" => invocation = Some(value.to_owned()),
            "ActiveState" => active = Some(value.to_owned()),
            _ => {}
        }
    }
    Some(SystemUnitState {
        invocation: invocation?,
        active: active?,
    })
}
