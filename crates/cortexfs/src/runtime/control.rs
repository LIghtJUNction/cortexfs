//! Internal capability channel for one active agent run.

use crate::{
    PeerCredentials, peer_credentials,
    support::{
        plain::open_plain_directory,
        receipt::{SocketReceipt, random_hex},
    },
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write as _};
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{os::unix::net::UnixListener, os::unix::net::UnixStream};

const SOCKET_NONCE_BYTES: usize = 16;
const MAX_FRAME_BYTES: u64 = 16 * 1024;
const MAX_REQUEST_IDS: usize = 64;
const MAX_REQUEST_ID_BYTES: usize = 128;
const MAX_ANCESTRY_DEPTH: usize = 64;

pub struct RunCapability {
    agent: String,
    session: String,
    run: String,
    uid: u32,
    gid: u32,
    socket_receipt: SocketReceipt,
    source_receipt: Option<cortexfs_runtime_client::RuntimeSourceReceipt>,
    roots: Mutex<Vec<LaunchRoot>>,
    #[cfg(test)]
    consumed: AtomicBool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct LaunchRoot {
    pid: u32,
    start_time: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ProcessStat {
    parent: u32,
    start_time: u64,
}

/// A one-use bwrap startup gate paired with its host-owned capability.
///
/// The read side is inherited only by bwrap through `--block-fd`. The host
/// registers the spawned bwrap PID before dropping the write side, so no
/// sandbox process can connect during the spawn-to-registration interval.
pub(crate) struct LaunchGate {
    read: Option<OwnedFd>,
    release: Option<OwnedFd>,
    capability: Arc<RunCapability>,
}

impl LaunchGate {
    pub(crate) fn block_fd(&self) -> RawFd {
        self.read.as_ref().map_or(-1, AsRawFd::as_raw_fd)
    }

    /// Authorizes the just-spawned bwrap host PID before unblocking it.
    pub(crate) fn register_and_release(&mut self, pid: u32) -> Result<(), RunCapabilityError> {
        if self.read.is_none() || self.release.is_none() {
            return Err(RunCapabilityError::CannotCreate);
        }
        self.capability.register_launch_root(pid)?;
        self.read.take();
        self.release.take();
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum RunCapabilityError {
    #[error("cannot create run capability")]
    CannotCreate,
    #[error("cannot accept run capability connection")]
    CannotAccept,
    #[error("cannot read run capability frame")]
    CannotRead,
    #[error("cannot write run capability frame")]
    CannotWrite,
    #[error("invalid run capability frame")]
    InvalidFrame,
    #[error("run capability peer denied")]
    PeerDenied,
    #[error("run capability active run changed")]
    RunChanged,
    #[error("run capability already consumed")]
    Replayed,
    #[error("run capability request set full")]
    RequestSetFull,
    #[error("run capability operation unsupported")]
    Unsupported,
    #[error("run capability cleanup conflict")]
    CleanupConflict,
}

impl RunCapabilityError {
    #[must_use]
    pub const fn errno(&self) -> &'static str {
        match *self {
            Self::Replayed => "EALREADY",
            Self::RequestSetFull => "ENOSPC",
            Self::Unsupported => "ENOSYS",
            Self::PeerDenied => "EACCES",
            Self::InvalidFrame => "EINVAL",
            Self::RunChanged => "ECANCELED",
            Self::CannotCreate
            | Self::CannotAccept
            | Self::CannotRead
            | Self::CannotWrite
            | Self::CleanupConflict => "EIO",
        }
    }
}

use cortexfs_runtime_client::{RequestFrame, ResponseFrame};

/// Strict authority-free payload for one parent-owned child creation request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreateChildRequest {
    pub agent: String,
    pub session: String,
    pub run: String,
    pub child: String,
    pub child_session: String,
    pub path: Option<String>,
    pub window: Option<u32>,
    pub input: String,
    pub life: String,
}

/// Strict authority-free payload for one self prompt-control update request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpdatePromptRequest {
    pub agent: String,
    pub session: String,
    pub run: String,
    pub control: String,
    pub content: String,
}

/// Stable successful child creation response.
pub use cortexfs_runtime_client::{
    CreateChildEnvironmentRequest, CreateChildResult, UpdatePromptEnvironmentRequest,
};

impl RunCapability {
    #[expect(
        clippy::too_many_arguments,
        reason = "capability constructor keeps source and peer identity fields explicit"
    )]
    pub fn create_with_source(
        directory: &Path,
        source: &Path,
        agent: &str,
        session: &str,
        run: &str,
        uid: u32,
        gid: u32,
    ) -> Result<(Self, UnixListener), RunCapabilityError> {
        let source_fd =
            open_plain_directory(source).map_err(|_error| RunCapabilityError::CannotCreate)?;
        let metadata = source_fd
            .metadata()
            .map_err(|_error| RunCapabilityError::CannotCreate)?;
        let (mut capability, listener) = Self::create(directory, agent, session, run, uid, gid)?;
        capability.source_receipt = Some(cortexfs_runtime_client::RuntimeSourceReceipt {
            path: source.display().to_string(),
            dev: metadata.dev(),
            ino: metadata.ino(),
            kind: cortexfs_runtime_client::RuntimeSourceKind::PlainDirectory,
        });
        Ok((capability, listener))
    }

    pub fn create(
        directory: &Path,
        agent: &str,
        session: &str,
        run: &str,
        uid: u32,
        gid: u32,
    ) -> Result<(Self, UnixListener), RunCapabilityError> {
        let nonce = random_hex::<SOCKET_NONCE_BYTES>()
            .map_err(|_error| RunCapabilityError::CannotCreate)?;
        let socket = directory.join(format!(
            "control-{}.sock",
            nonce.get(..24).unwrap_or(&nonce)
        ));
        let name = socket
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(RunCapabilityError::CannotCreate)?;
        let (socket_receipt, listener) =
            SocketReceipt::bind(directory, name, (uid, gid)).map_err(|error| match error {
                crate::support::receipt::SocketReceiptError::Create => {
                    RunCapabilityError::CannotCreate
                }
                crate::support::receipt::SocketReceiptError::Cleanup => {
                    RunCapabilityError::CleanupConflict
                }
            })?;
        Ok((
            Self {
                agent: agent.to_owned(),
                session: session.to_owned(),
                run: run.to_owned(),
                uid,
                gid,
                socket_receipt,
                source_receipt: None,
                roots: Mutex::new(Vec::new()),
                #[cfg(test)]
                consumed: AtomicBool::new(false),
            },
            listener,
        ))
    }

    #[must_use]
    pub fn socket(&self) -> &Path {
        self.socket_receipt.path()
    }

    #[must_use]
    pub fn environment(sandbox_socket: &Path) -> [(String, String); 1] {
        [(
            "CTX_CONTROL_SOCKET".to_owned(),
            sandbox_socket.display().to_string(),
        )]
    }

    pub(crate) fn launch_gate(self: &Arc<Self>) -> Result<LaunchGate, RunCapabilityError> {
        let (read, release) =
            nix::unistd::pipe().map_err(|_error| RunCapabilityError::CannotCreate)?;
        let flags = nix::fcntl::fcntl(&release, nix::fcntl::FcntlArg::F_GETFD)
            .map_err(|_error| RunCapabilityError::CannotCreate)?;
        let flags = nix::fcntl::FdFlag::from_bits_truncate(flags) | nix::fcntl::FdFlag::FD_CLOEXEC;
        nix::fcntl::fcntl(&release, nix::fcntl::FcntlArg::F_SETFD(flags))
            .map_err(|_error| RunCapabilityError::CannotCreate)?;
        Ok(LaunchGate {
            read: Some(read),
            release: Some(release),
            capability: Arc::clone(self),
        })
    }

    /// Registers the host PID that owns a deliberately launched sandbox tree.
    pub fn register_launch_root(&self, pid: u32) -> Result<(), RunCapabilityError> {
        let stat = read_process_stat(pid).ok_or(RunCapabilityError::CannotCreate)?;
        {
            let mut roots = self
                .roots
                .lock()
                .map_err(|_error| RunCapabilityError::CannotCreate)?;
            roots.retain(|root| root.pid != pid);
            roots.push(LaunchRoot {
                pid,
                start_time: stat.start_time,
            });
        }
        Ok(())
    }

    pub fn serve_run(
        &self,
        listener: &UnixListener,
        shutdown: &AtomicBool,
        startup: &SyncSender<Result<(), RunCapabilityError>>,
        current_run: impl FnMut() -> Option<String>,
    ) -> Result<(), RunCapabilityError> {
        self.serve_run_with_handler(
            listener,
            shutdown,
            startup,
            current_run,
            |_request| Err(RunCapabilityError::Unsupported),
            |_request| Err(RunCapabilityError::Unsupported),
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "run server keeps its per-operation handlers explicit"
    )]
    pub fn serve_run_with_handler(
        &self,
        listener: &UnixListener,
        shutdown: &AtomicBool,
        startup: &SyncSender<Result<(), RunCapabilityError>>,
        mut current_run: impl FnMut() -> Option<String>,
        mut create_child: impl FnMut(
            CreateChildRequest,
        ) -> Result<CreateChildResult, RunCapabilityError>,
        mut update_prompt: impl FnMut(UpdatePromptRequest) -> Result<(), RunCapabilityError>,
    ) -> Result<(), RunCapabilityError> {
        listener
            .set_nonblocking(true)
            .map_err(|_error| RunCapabilityError::CannotAccept)?;
        let mut seen = HashSet::new();
        let mut startup_sent = false;
        let startup_id = format!("startup-{}", self.run);
        while !shutdown.load(Ordering::Acquire) {
            match listener.accept() {
                Ok((mut stream, _address)) => {
                    let result = self.handle_connection(
                        &mut stream,
                        &mut seen,
                        &mut current_run,
                        &mut create_child,
                        &mut update_prompt,
                    );
                    if !startup_sent && result.as_deref() == Ok(startup_id.as_str()) {
                        let _ignored = startup.send(Ok(()));
                        startup_sent = true;
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_error) => return Err(RunCapabilityError::CannotAccept),
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn serve_ping(
        &self,
        listener: &UnixListener,
        mut current_run: impl FnMut() -> Option<String>,
    ) -> Result<(), RunCapabilityError> {
        if self.consumed.load(Ordering::Acquire) {
            return Err(RunCapabilityError::Replayed);
        }
        if current_run().as_deref() != Some(self.run.as_str()) {
            return Err(RunCapabilityError::RunChanged);
        }
        listener
            .set_nonblocking(true)
            .map_err(|_error| RunCapabilityError::CannotAccept)?;
        let deadline = std::time::Instant::now() + control_timeout();
        let (mut stream, _address) = loop {
            match listener.accept() {
                Ok(connection) => break connection,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && std::time::Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_error) => return Err(RunCapabilityError::CannotAccept),
            }
        };
        let mut seen = HashSet::new();
        self.handle_connection(
            &mut stream,
            &mut seen,
            &mut current_run,
            &mut |_request| Err(RunCapabilityError::Unsupported),
            &mut |_request| Err(RunCapabilityError::Unsupported),
        )?;
        self.consumed.store(true, Ordering::Release);
        Ok(())
    }

    fn handle_connection(
        &self,
        stream: &mut UnixStream,
        seen: &mut HashSet<String>,
        current_run: &mut impl FnMut() -> Option<String>,
        create_child: &mut impl FnMut(
            CreateChildRequest,
        ) -> Result<CreateChildResult, RunCapabilityError>,
        update_prompt: &mut impl FnMut(UpdatePromptRequest) -> Result<(), RunCapabilityError>,
    ) -> Result<String, RunCapabilityError> {
        if current_run().as_deref() != Some(self.run.as_str()) {
            return Err(RunCapabilityError::RunChanged);
        }
        stream
            .set_read_timeout(Some(control_timeout()))
            .and_then(|()| stream.set_write_timeout(Some(control_timeout())))
            .map_err(|_error| RunCapabilityError::CannotRead)?;
        let peer = peer_credentials(stream).map_err(|_error| RunCapabilityError::PeerDenied)?;
        if !peer_allowed(peer, self.uid, &self.roots) {
            return Err(RunCapabilityError::PeerDenied);
        }
        let frame: RequestFrame = read_json_line(stream)?;
        let request_id = match frame {
            RequestFrame::Ping {
                request_id,
                agent,
                session,
                run,
                ..
            } => {
                if agent != self.agent || session != self.session || run != self.run {
                    return Err(RunCapabilityError::InvalidFrame);
                }
                request_id
            }
            RequestFrame::CreateChild {
                request_id,
                agent,
                session,
                run,
                child,
                child_session,
                path,
                window,
                input,
                life,
                ..
            } => {
                self.authorize_request(stream, seen, &request_id, &mut *current_run)?;
                if agent != self.agent || session != self.session || run != self.run {
                    let _ignored = write_error_frame(stream, request_id, "EINVAL");
                    return Err(RunCapabilityError::InvalidFrame);
                }
                if crate::ChildLifecycle::parse_exact(&life).is_err() {
                    let _ignored = write_error_frame(stream, request_id, "EINVAL");
                    return Err(RunCapabilityError::InvalidFrame);
                }
                if window == Some(0) {
                    let _ignored = write_error_frame(stream, request_id, "EINVAL");
                    return Err(RunCapabilityError::InvalidFrame);
                }
                let result = create_child(CreateChildRequest {
                    agent,
                    session,
                    run,
                    child,
                    child_session,
                    path,
                    window,
                    input,
                    life,
                });
                return match result {
                    Ok(result) => {
                        write_frame(
                            stream,
                            &ResponseFrame::ChildCreated {
                                request_id: request_id.clone(),
                                result,
                            },
                        )?;
                        Ok(request_id)
                    }
                    Err(error) => {
                        let _ignored = write_error_frame(stream, request_id, error.errno());
                        Err(error)
                    }
                };
            }
            RequestFrame::UpdatePrompt {
                request_id,
                agent,
                session,
                run,
                control,
                content,
                ..
            } => {
                self.authorize_request(stream, seen, &request_id, &mut *current_run)?;
                if agent != self.agent
                    || session != self.session
                    || run != self.run
                    || !cortexfs_runtime_client::is_agent_prompt_control(&control)
                    || content.len() > cortexfs_runtime_client::MAX_SELF_UPDATE_CONTENT_BYTES
                {
                    let _ignored = write_error_frame(stream, request_id, "EINVAL");
                    return Err(RunCapabilityError::InvalidFrame);
                }
                let request = UpdatePromptRequest {
                    agent,
                    session,
                    run,
                    control,
                    content,
                };
                return respond_update_prompt(stream, request_id, update_prompt(request));
            }
        };
        self.authorize_request(stream, seen, &request_id, &mut *current_run)?;
        write_frame(
            stream,
            &ResponseFrame::Pong {
                request_id: request_id.clone(),
                receipt: self.source_receipt.clone(),
            },
        )?;
        Ok(request_id)
    }

    fn authorize_request(
        &self,
        stream: &mut UnixStream,
        seen: &mut HashSet<String>,
        request_id: &str,
        current_run: &mut dyn FnMut() -> Option<String>,
    ) -> Result<(), RunCapabilityError> {
        if !legal_request_id(request_id) {
            return Err(RunCapabilityError::InvalidFrame);
        }
        if current_run().as_deref() != Some(self.run.as_str()) {
            return Err(RunCapabilityError::RunChanged);
        }
        reserve_request_id(stream, seen, request_id)
    }

    pub fn ping(&self, request_id: &str) -> Result<(), RunCapabilityError> {
        cortexfs_runtime_client::ping(
            self.socket_receipt.path(),
            "",
            request_id,
            &self.agent,
            &self.session,
            &self.run,
        )
        .map(|_| ())
        .map_err(client_error)
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "wire helper keeps capability and requested child controls explicit"
    )]
    pub fn create_child(
        &self,
        request_id: &str,
        child: &str,
        child_session: &str,
        path: Option<&str>,
        window: Option<u32>,
        input: &str,
        life: &str,
    ) -> Result<CreateChildResult, RunCapabilityError> {
        cortexfs_runtime_client::create_child(
            self.socket_receipt.path(),
            "",
            request_id,
            &self.agent,
            &self.session,
            &self.run,
            child,
            child_session,
            path,
            window,
            input,
            life,
        )
        .map_err(client_error)
    }

    pub fn update_prompt(
        &self,
        request_id: &str,
        control: &str,
        content: &str,
    ) -> Result<(), RunCapabilityError> {
        cortexfs_runtime_client::update_prompt(
            self.socket_receipt.path(),
            "",
            request_id,
            &self.agent,
            &self.session,
            &self.run,
            control,
            content,
        )
        .map_err(client_error)
    }

    pub fn cleanup(&self) -> Result<(), RunCapabilityError> {
        self.socket_receipt
            .cleanup()
            .map_err(|_error| RunCapabilityError::CleanupConflict)
    }
}

fn error_from_errno(errno: &str) -> RunCapabilityError {
    match errno {
        "EALREADY" => RunCapabilityError::Replayed,
        "ENOSPC" => RunCapabilityError::RequestSetFull,
        "ENOSYS" => RunCapabilityError::Unsupported,
        "EACCES" => RunCapabilityError::PeerDenied,
        "ECANCELED" => RunCapabilityError::RunChanged,
        "EIO" => RunCapabilityError::CannotCreate,
        _ => RunCapabilityError::InvalidFrame,
    }
}

pub fn create_child_from_environment(
    request: CreateChildEnvironmentRequest<'_>,
) -> Result<CreateChildResult, RunCapabilityError> {
    cortexfs_runtime_client::create_child_from_environment(request).map_err(client_error)
}

pub fn update_prompt_from_environment(
    request: UpdatePromptEnvironmentRequest<'_>,
) -> Result<(), RunCapabilityError> {
    cortexfs_runtime_client::update_prompt_from_environment(request).map_err(client_error)
}

/// Performs the optional one-shot runner handshake from reserved environment.
/// Absence means the direct/legacy execution path; partial or invalid values fail closed.
pub fn ping_from_environment(agent: &str) -> Result<(), RunCapabilityError> {
    cortexfs_runtime_client::ping_from_environment(agent)
        .map(|_| ())
        .map_err(client_error)
}

fn client_error(error: cortexfs_runtime_client::RuntimeClientError) -> RunCapabilityError {
    match error {
        cortexfs_runtime_client::RuntimeClientError::InvalidEnvironment
        | cortexfs_runtime_client::RuntimeClientError::InvalidRequest
        | cortexfs_runtime_client::RuntimeClientError::InvalidFrame => {
            RunCapabilityError::InvalidFrame
        }
        cortexfs_runtime_client::RuntimeClientError::CannotConnect => {
            RunCapabilityError::CannotAccept
        }
        cortexfs_runtime_client::RuntimeClientError::CannotWrite => RunCapabilityError::CannotWrite,
        cortexfs_runtime_client::RuntimeClientError::CannotRead => RunCapabilityError::CannotRead,
        cortexfs_runtime_client::RuntimeClientError::Rejected(errno) => error_from_errno(&errno),
    }
}

fn peer_allowed(peer: PeerCredentials, uid: u32, roots: &Mutex<Vec<LaunchRoot>>) -> bool {
    let Ok(roots) = roots.lock() else {
        return false;
    };
    peer_allowed_with(peer, uid, &roots, read_process_stat)
}

fn peer_allowed_with(
    peer: PeerCredentials,
    uid: u32,
    roots: &[LaunchRoot],
    mut read_stat: impl FnMut(u32) -> Option<ProcessStat>,
) -> bool {
    if peer.uid() != uid {
        return false;
    }
    let Some(pid) = peer.pid().and_then(|value| u32::try_from(value).ok()) else {
        return false;
    };
    roots
        .iter()
        .copied()
        .any(|root| process_descends_from(pid, root, &mut read_stat))
}

fn process_descends_from(
    pid: u32,
    root: LaunchRoot,
    mut read_stat: impl FnMut(u32) -> Option<ProcessStat>,
) -> bool {
    let mut current = pid;
    let mut seen = HashSet::new();
    for _ in 0..MAX_ANCESTRY_DEPTH {
        if !seen.insert(current) {
            return false;
        }
        let Some(stat) = read_stat(current) else {
            return false;
        };
        if current == root.pid {
            return stat.start_time == root.start_time && !seen.contains(&stat.parent);
        }
        if stat.parent == 0 || stat.parent == current {
            return false;
        }
        current = stat.parent;
    }
    false
}

fn read_process_stat(pid: u32) -> Option<ProcessStat> {
    if pid == 0 {
        return None;
    }
    parse_process_stat(&fs::read_to_string(format!("/proc/{pid}/stat")).ok()?)
}

fn parse_process_stat(stat: &str) -> Option<ProcessStat> {
    let closing = stat.rfind(')')?;
    let fields = stat
        .get(closing.checked_add(1)?..)?
        .split_ascii_whitespace();
    let fields = fields.collect::<Vec<_>>();
    let state = *fields.first()?;
    if matches!(state, "Z" | "X") {
        return None;
    }
    let parent = fields.get(1)?.parse().ok()?;
    let start_time = fields.get(19)?.parse().ok()?;
    Some(ProcessStat { parent, start_time })
}

fn control_timeout() -> Duration {
    if cfg!(test) {
        Duration::from_millis(100)
    } else {
        Duration::from_secs(5)
    }
}

fn legal_request_id(request_id: &str) -> bool {
    !request_id.is_empty()
        && request_id.len() <= MAX_REQUEST_ID_BYTES
        && request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn respond_update_prompt(
    stream: &mut UnixStream,
    request_id: String,
    result: Result<(), RunCapabilityError>,
) -> Result<String, RunCapabilityError> {
    match result {
        Ok(()) => {
            write_frame(
                stream,
                &ResponseFrame::PromptUpdated {
                    request_id: request_id.clone(),
                },
            )?;
            Ok(request_id)
        }
        Err(error) => {
            let _ignored = write_error_frame(stream, request_id, error.errno());
            Err(error)
        }
    }
}

fn write_error_frame(
    stream: &mut UnixStream,
    request_id: String,
    errno: &'static str,
) -> Result<(), RunCapabilityError> {
    write_frame(
        stream,
        &ResponseFrame::Error {
            request_id,
            errno: errno.to_owned(),
        },
    )
}

fn reserve_request_id(
    stream: &mut UnixStream,
    seen: &mut HashSet<String>,
    request_id: &str,
) -> Result<(), RunCapabilityError> {
    if seen.contains(request_id) {
        let _ignored = write_error_frame(stream, request_id.to_owned(), "EALREADY");
        return Err(RunCapabilityError::Replayed);
    }
    if seen.len() == MAX_REQUEST_IDS {
        let _ignored = write_error_frame(stream, request_id.to_owned(), "ENOSPC");
        return Err(RunCapabilityError::RequestSetFull);
    }
    seen.insert(request_id.to_owned());
    Ok(())
}

/// Reads one newline-terminated JSON frame from the unix stream and decodes it.
fn read_json_line<T: for<'de> Deserialize<'de>>(
    stream: &mut UnixStream,
) -> Result<T, RunCapabilityError> {
    let mut bytes = Vec::new();
    BufReader::new(stream)
        .take(MAX_FRAME_BYTES + 1)
        .read_until(b'\n', &mut bytes)
        .map_err(|_error| RunCapabilityError::CannotRead)?;
    if bytes.is_empty() || u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_FRAME_BYTES {
        return Err(RunCapabilityError::InvalidFrame);
    }
    if bytes.last() != Some(&b'\n') {
        return Err(RunCapabilityError::InvalidFrame);
    }
    serde_json::from_slice(&bytes).map_err(|_error| RunCapabilityError::InvalidFrame)
}

fn write_frame(stream: &mut UnixStream, frame: &impl Serialize) -> Result<(), RunCapabilityError> {
    serde_json::to_writer(&mut *stream, frame).map_err(|_error| RunCapabilityError::CannotWrite)?;
    stream
        .write_all(b"\n")
        .map_err(|_error| RunCapabilityError::CannotWrite)
}

impl fmt::Debug for RunCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("RunCapability");
        debug
            .field("socket", &self.socket_receipt.path())
            .field("agent", &self.agent)
            .field("session", &self.session)
            .field("run", &self.run)
            .field("uid", &self.uid)
            .field("gid", &self.gid)
            .field("dev", &self.socket_receipt.identity().0)
            .field("ino", &self.socket_receipt.identity().1)
            .field("source_receipt", &self.source_receipt);
        if let Ok(roots) = self.roots.lock() {
            debug.field("launch_roots", &roots.len());
        }
        #[cfg(test)]
        debug.field("consumed", &self.consumed.load(Ordering::Acquire));
        debug.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{FileTypeExt, PermissionsExt};
    use std::sync::Arc;
    use std::thread;

    type Fixture = (tempfile::TempDir, Arc<RunCapability>, UnixListener);

    fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o711))?;
        let (capability, listener) = RunCapability::create(
            root.path(),
            "parent",
            "session-1",
            "run-1",
            nix::unistd::getuid().as_raw(),
            nix::unistd::getgid().as_raw(),
        )?;
        let capability = Arc::new(capability);
        // These unit tests exercise the in-process protocol only. The Bwrap
        // execution proof registers a spawned host root through a gate.
        capability.register_launch_root(std::process::id())?;
        Ok((root, capability, listener))
    }

    fn serve(
        capability: Arc<RunCapability>,
        listener: UnixListener,
        run: &'static str,
    ) -> thread::JoinHandle<Result<(), RunCapabilityError>> {
        thread::spawn(move || capability.serve_ping(&listener, || Some(run.to_owned())))
    }

    fn join_server(
        server: thread::JoinHandle<Result<(), RunCapabilityError>>,
    ) -> std::io::Result<Result<(), RunCapabilityError>> {
        server
            .join()
            .map_err(|_error| std::io::Error::other("server panicked"))
    }

    fn send_raw(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
        let mut stream = UnixStream::connect(path)?;
        stream.write_all(bytes)?;
        stream.shutdown(std::net::Shutdown::Write)
    }

    #[test]
    fn created_socket_is_receipt_bound_and_owner_private() -> Result<(), Box<dyn std::error::Error>>
    {
        let (_root, capability, _listener) = fixture()?;
        let metadata = fs::symlink_metadata(capability.socket())?;

        assert!(metadata.file_type().is_socket());
        assert_eq!(metadata.uid(), capability.uid);
        assert_eq!(metadata.gid(), capability.gid);
        assert_eq!(metadata.permissions().mode() & 0o7777, 0o600);
        assert_eq!(
            (metadata.dev(), metadata.ino()),
            capability.socket_receipt.identity()
        );
        Ok(())
    }

    #[test]
    fn one_shot_ping_binds_run_and_rejects_replay() -> Result<(), Box<dyn std::error::Error>> {
        let (root, capability, listener) = fixture()?;
        let server_capability = Arc::clone(&capability);
        let server = thread::spawn(move || {
            server_capability.serve_ping(&listener, || Some("run-1".to_owned()))
        });
        capability.ping("request-1")?;
        let server_result = server
            .join()
            .map_err(|_error| std::io::Error::other("capability server panicked"))?;
        assert_eq!(server_result, Ok(()));
        assert!(capability.consumed.load(Ordering::Acquire));
        let replay_listener = UnixListener::bind(root.path().join("replay.sock"))?;
        assert_eq!(
            capability.serve_ping(&replay_listener, || Some("run-1".to_owned())),
            Err(RunCapabilityError::Replayed)
        );
        capability.cleanup()?;
        Ok(())
    }

    #[test]
    fn cleanup_refuses_replacement() -> Result<(), Box<dyn std::error::Error>> {
        let (_root, capability, listener) = fixture()?;
        drop(listener);
        fs::remove_file(capability.socket())?;
        UnixListener::bind(capability.socket())?;
        assert_eq!(
            capability.cleanup(),
            Err(RunCapabilityError::CleanupConflict)
        );
        assert!(capability.socket().exists());
        Ok(())
    }

    #[test]
    fn legacy_token_is_ignored_and_wrong_run_is_rejected() -> Result<(), Box<dyn std::error::Error>>
    {
        let (_root, capability, listener) = fixture()?;
        let server = serve(Arc::clone(&capability), listener, "run-1");
        let frame = RequestFrame::Ping {
            token: "legacy-token".to_owned(),
            request_id: "request-1".to_owned(),
            agent: "parent".to_owned(),
            session: "session-1".to_owned(),
            run: "run-1".to_owned(),
        };
        let mut bytes = serde_json::to_vec(&frame)?;
        bytes.push(b'\n');
        let mut stream = UnixStream::connect(capability.socket())?;
        stream.write_all(&bytes)?;
        assert!(matches!(
            read_json_line::<ResponseFrame>(&mut stream)?,
            ResponseFrame::Pong { request_id, .. } if request_id == "request-1"
        ));
        assert_eq!(join_server(server)?, Ok(()));

        let (_root, capability, listener) = fixture()?;
        assert_eq!(
            capability.serve_ping(&listener, || Some("other-run".to_owned())),
            Err(RunCapabilityError::RunChanged)
        );
        Ok(())
    }

    #[test]
    fn partial_and_oversized_frames_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let (_root, capability, listener) = fixture()?;
        let server = serve(Arc::clone(&capability), listener, "run-1");
        send_raw(capability.socket(), br#"{"op":"ping"}"#)?;
        assert_eq!(join_server(server)?, Err(RunCapabilityError::InvalidFrame));

        let (_root, capability, listener) = fixture()?;
        let server = serve(Arc::clone(&capability), listener, "run-1");
        let oversized = vec![b'x'; usize::try_from(MAX_FRAME_BYTES)? + 1];
        send_raw(capability.socket(), &oversized)?;
        assert_eq!(join_server(server)?, Err(RunCapabilityError::InvalidFrame));
        Ok(())
    }

    #[test]
    fn tokenless_frame_and_changed_run_are_checked() -> Result<(), Box<dyn std::error::Error>> {
        let (_root, capability, listener) = fixture()?;
        let server = serve(Arc::clone(&capability), listener, "run-1");
        let mut stream = UnixStream::connect(capability.socket())?;
        stream.write_all(
            br#"{"op":"ping","request_id":"request-1","agent":"parent","session":"session-1","run":"run-1"}
"#,
        )?;
        assert!(matches!(
            read_json_line::<ResponseFrame>(&mut stream)?,
            ResponseFrame::Pong { request_id, .. } if request_id == "request-1"
        ));
        assert_eq!(join_server(server)?, Ok(()));

        let (_root, capability, listener) = fixture()?;
        let server = serve(Arc::clone(&capability), listener, "run-1");
        let frame = RequestFrame::Ping {
            token: String::new(),
            request_id: "request-2".to_owned(),
            agent: "parent".to_owned(),
            session: "session-1".to_owned(),
            run: "wrong-run".to_owned(),
        };
        let mut bytes = serde_json::to_vec(&frame)?;
        bytes.push(b'\n');
        send_raw(capability.socket(), &bytes)?;
        assert_eq!(join_server(server)?, Err(RunCapabilityError::InvalidFrame));

        let (_root, capability, listener) = fixture()?;
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let server_calls = Arc::clone(&calls);
        let server_capability = Arc::clone(&capability);
        let server = thread::spawn(move || {
            server_capability.serve_ping(&listener, || {
                let call = server_calls.fetch_add(1, Ordering::AcqRel);
                Some(if call == 0 { "run-1" } else { "changed-run" }.to_owned())
            })
        });
        let frame = RequestFrame::Ping {
            token: String::new(),
            request_id: "request-2".to_owned(),
            agent: "parent".to_owned(),
            session: "session-1".to_owned(),
            run: "run-1".to_owned(),
        };
        let mut bytes = serde_json::to_vec(&frame)?;
        bytes.push(b'\n');
        send_raw(capability.socket(), &bytes)?;
        assert_eq!(join_server(server)?, Err(RunCapabilityError::RunChanged));
        Ok(())
    }

    #[test]
    fn peer_and_timeouts_are_bounded() -> Result<(), Box<dyn std::error::Error>> {
        let root = LaunchRoot {
            pid: 10,
            start_time: 100,
        };
        let stat = |pid| match pid {
            10 => Some(ProcessStat {
                parent: 1,
                start_time: 100,
            }),
            11 => Some(ProcessStat {
                parent: 10,
                start_time: 101,
            }),
            _ => None,
        };
        assert!(!peer_allowed_with(
            PeerCredentials::new(None, 1000, 1002),
            1000,
            &[root],
            stat,
        ));
        assert!(!peer_allowed_with(
            PeerCredentials::new(Some(11), 1001, 1002),
            1000,
            &[root],
            stat,
        ));
        assert!(peer_allowed_with(
            PeerCredentials::new(Some(11), 1000, 1002),
            1000,
            &[root],
            stat,
        ));
        assert!(!peer_allowed_with(
            PeerCredentials::new(Some(99), 1000, 1002),
            1000,
            &[root],
            stat,
        ));
        let (_root, capability, listener) = fixture()?;
        assert_eq!(
            capability.serve_ping(&listener, || Some("run-1".to_owned())),
            Err(RunCapabilityError::CannotAccept)
        );

        let (_root, capability, listener) = fixture()?;
        let server = serve(Arc::clone(&capability), listener, "run-1");
        let _idle = UnixStream::connect(capability.socket())?;
        assert_eq!(join_server(server)?, Err(RunCapabilityError::CannotRead));
        Ok(())
    }

    #[test]
    fn launch_root_requires_live_descendant_and_matching_start_time() {
        let root = LaunchRoot {
            pid: 10,
            start_time: 100,
        };
        let live_tree = |pid| match pid {
            10 => Some(ProcessStat {
                parent: 1,
                start_time: 100,
            }),
            11 => Some(ProcessStat {
                parent: 10,
                start_time: 101,
            }),
            12 => Some(ProcessStat {
                parent: 11,
                start_time: 102,
            }),
            _ => None,
        };
        assert!(process_descends_from(10, root, live_tree));
        assert!(process_descends_from(12, root, live_tree));
        assert!(!process_descends_from(12, root, |pid| match pid {
            10 => Some(ProcessStat {
                parent: 1,
                start_time: 999,
            }),
            11 => Some(ProcessStat {
                parent: 10,
                start_time: 101,
            }),
            12 => Some(ProcessStat {
                parent: 11,
                start_time: 102,
            }),
            _ => None,
        }));
        assert!(!process_descends_from(12, root, |pid| match pid {
            10 => Some(ProcessStat {
                parent: 1,
                start_time: 100,
            }),
            11 => Some(ProcessStat {
                parent: 1,
                start_time: 101,
            }),
            12 => Some(ProcessStat {
                parent: 11,
                start_time: 102,
            }),
            _ => None,
        }));
        assert!(!process_descends_from(12, root, |pid| match pid {
            10 => Some(ProcessStat {
                parent: 11,
                start_time: 100,
            }),
            11 => Some(ProcessStat {
                parent: 10,
                start_time: 101,
            }),
            12 => Some(ProcessStat {
                parent: 11,
                start_time: 102,
            }),
            _ => None,
        }));
    }

    #[test]
    fn proc_stat_parser_uses_final_command_delimiter() {
        let stat = "42 (worker ) with spaces) S 7 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 1234 0";
        assert_eq!(
            parse_process_stat(stat),
            Some(ProcessStat {
                parent: 7,
                start_time: 1234,
            })
        );
        assert_eq!(parse_process_stat("42 (worker) Z 7 0"), None);
    }

    #[test]
    fn run_server_recovers_tracks_requests_and_shuts_down() -> Result<(), Box<dyn std::error::Error>>
    {
        let (_root, capability, listener) = fixture()?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = Arc::clone(&shutdown);
        let (startup_sender, startup_receiver) = std::sync::mpsc::sync_channel(1);
        let server_capability = Arc::clone(&capability);
        let server = thread::spawn(move || {
            server_capability.serve_run(&listener, &server_shutdown, &startup_sender, || {
                Some("run-1".to_owned())
            })
        });

        send_raw(capability.socket(), b"not-json\n")?;
        assert_eq!(capability.ping("startup-run-1"), Ok(()));
        assert_eq!(startup_receiver.recv()?, Ok(()));
        assert_eq!(capability.ping("request-2"), Ok(()));
        assert_eq!(
            capability.ping("request-2"),
            Err(RunCapabilityError::Replayed)
        );
        for index in 3..=64 {
            assert_eq!(capability.ping(&format!("request-{index}")), Ok(()));
        }
        assert_eq!(
            capability.ping("request-65"),
            Err(RunCapabilityError::RequestSetFull)
        );
        shutdown.store(true, Ordering::Release);
        assert_eq!(join_server(server)?, Ok(()));
        capability.cleanup()?;
        assert!(!capability.socket().exists());
        Ok(())
    }

    #[test]
    fn reserved_create_frame_is_strict_and_non_mutating() -> Result<(), Box<dyn std::error::Error>>
    {
        let (_root, capability, listener) = fixture()?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = Arc::clone(&shutdown);
        let (startup_sender, startup_receiver) = std::sync::mpsc::sync_channel(1);
        let server_capability = Arc::clone(&capability);
        let server = thread::spawn(move || {
            server_capability.serve_run(&listener, &server_shutdown, &startup_sender, || {
                Some("run-1".to_owned())
            })
        });
        capability.ping("startup-run-1")?;
        assert_eq!(startup_receiver.recv()?, Ok(()));

        let mut stream = UnixStream::connect(capability.socket())?;
        write_frame(
            &mut stream,
            &RequestFrame::CreateChild {
                token: String::new(),
                request_id: "create-1".to_owned(),
                agent: "parent".to_owned(),
                session: "session-1".to_owned(),
                run: "run-1".to_owned(),
                child: "child".to_owned(),
                child_session: "child-session".to_owned(),
                path: None,
                window: None,
                input: "work".to_owned(),
                life: "owned".to_owned(),
            },
        )?;
        let error: ResponseFrame = read_json_line(&mut stream)?;
        assert!(matches!(
            error,
            ResponseFrame::Error { request_id, errno }
                if request_id == "create-1" && errno == "ENOSYS"
        ));

        send_raw(
            capability.socket(),
            br#"{"op":"agent.create","request_id":"create-2","agent":"parent","session":"session-1","run":"run-1","child":"child","child_session":"child-session","input":"work"}
"#,
        )?;
        assert_eq!(capability.ping("request-after-invalid"), Ok(()));
        shutdown.store(true, Ordering::Release);
        assert_eq!(join_server(server)?, Ok(()));
        capability.cleanup()?;
        Ok(())
    }

    fn assert_zero_window_create_rejected(
        capability: &RunCapability,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut zero_stream = UnixStream::connect(capability.socket())?;
        write_frame(
            &mut zero_stream,
            &RequestFrame::CreateChild {
                token: String::new(),
                request_id: "create-zero".to_owned(),
                agent: "parent".to_owned(),
                session: "session-1".to_owned(),
                run: "run-1".to_owned(),
                child: "child-zero".to_owned(),
                child_session: "session-zero".to_owned(),
                path: None,
                window: Some(0),
                input: "work".to_owned(),
                life: "owned".to_owned(),
            },
        )?;
        assert!(matches!(
            read_json_line::<ResponseFrame>(&mut zero_stream)?,
            ResponseFrame::Error { errno, .. } if errno == "EINVAL"
        ));
        Ok(())
    }

    #[test]
    fn create_handler_runs_once_per_unique_request() -> Result<(), Box<dyn std::error::Error>> {
        use std::sync::atomic::AtomicUsize;
        let (_root, capability, listener) = fixture()?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = Arc::clone(&shutdown);
        let calls = Arc::new(AtomicUsize::new(0));
        let server_calls = Arc::clone(&calls);
        let windows = Arc::new(Mutex::new(Vec::new()));
        let server_windows = Arc::clone(&windows);
        let (startup_sender, startup_receiver) = std::sync::mpsc::sync_channel(1);
        let server_capability = Arc::clone(&capability);
        let server = thread::spawn(move || {
            server_capability.serve_run_with_handler(
                &listener,
                &server_shutdown,
                &startup_sender,
                || Some("run-1".to_owned()),
                move |request| {
                    server_calls.fetch_add(1, Ordering::AcqRel);
                    server_windows
                        .lock()
                        .map_err(|_error| RunCapabilityError::CannotCreate)?
                        .push(request.window);
                    Ok(CreateChildResult {
                        child: request.child,
                        child_session: request.child_session,
                        pid: 42,
                    })
                },
                |_request| Err(RunCapabilityError::Unsupported),
            )
        });
        capability.ping("startup-run-1")?;
        assert_eq!(startup_receiver.recv()?, Ok(()));
        assert_zero_window_create_rejected(&capability)?;
        assert_eq!(calls.load(Ordering::Acquire), 0);
        for (request_id, life) in [
            ("create-padded-life", " temp "),
            ("create-unknown-life", "detached"),
        ] {
            assert_eq!(
                capability.create_child(request_id, "invalid", "invalid", None, None, "work", life),
                Err(RunCapabilityError::InvalidFrame)
            );
        }
        assert_eq!(calls.load(Ordering::Acquire), 0);
        assert_eq!(
            capability.create_child(
                "create-1",
                "child-a",
                "session-a",
                None,
                None,
                "work",
                "owned",
            )?,
            CreateChildResult {
                child: "child-a".to_owned(),
                child_session: "session-a".to_owned(),
                pid: 42,
            }
        );
        assert_eq!(
            capability.create_child(
                "create-1",
                "child-a",
                "session-a",
                None,
                None,
                "work",
                "owned",
            ),
            Err(RunCapabilityError::Replayed)
        );
        assert_eq!(
            capability
                .create_child(
                    "create-2",
                    "child-b",
                    "session-b",
                    None,
                    None,
                    "other",
                    "temp",
                )?
                .pid,
            42
        );
        assert_eq!(
            capability
                .create_child(
                    "create-3",
                    "child-c",
                    "session-c",
                    None,
                    Some(2048),
                    "windowed",
                    "owned",
                )?
                .pid,
            42
        );
        assert_eq!(calls.load(Ordering::Acquire), 3);
        assert_eq!(
            *windows
                .lock()
                .map_err(|_error| std::io::Error::other("window capture poisoned"))?,
            [None, None, Some(2048)]
        );
        shutdown.store(true, Ordering::Release);
        assert_eq!(join_server(server)?, Ok(()));
        capability.cleanup()?;
        Ok(())
    }

    /// 校验 agent.update 客户端输入筛选和服务端身份绑定都 fail closed，
    /// 合法的本 agent prompt 请求恰好调用 handler 一次。
    #[test]
    fn update_handler_applies_only_self_prompt_controls() -> Result<(), Box<dyn std::error::Error>>
    {
        let (_root, capability, listener) = fixture()?;
        let shutdown = Arc::new(AtomicBool::new(false));
        let server_shutdown = Arc::clone(&shutdown);
        let updates = Arc::new(Mutex::new(Vec::new()));
        let server_updates = Arc::clone(&updates);
        let (startup_sender, startup_receiver) = std::sync::mpsc::sync_channel(1);
        let server_capability = Arc::clone(&capability);
        let server = thread::spawn(move || {
            server_capability.serve_run_with_handler(
                &listener,
                &server_shutdown,
                &startup_sender,
                || Some("run-1".to_owned()),
                |_request| Err(RunCapabilityError::Unsupported),
                move |request| {
                    server_updates
                        .lock()
                        .map_err(|_error| RunCapabilityError::CannotWrite)?
                        .push((request.control, request.content));
                    Ok(())
                },
            )
        });
        capability.ping("startup-run-1")?;
        assert_eq!(startup_receiver.recv()?, Ok(()));
        assert_eq!(
            capability.update_prompt("update-denied", "policy", "allow coder_t tool:tsh execute"),
            Err(RunCapabilityError::InvalidFrame)
        );
        let oversized =
            "x".repeat(cortexfs_runtime_client::MAX_SELF_UPDATE_CONTENT_BYTES.saturating_add(1));
        assert_eq!(
            capability.update_prompt("update-oversized", "system.md", &oversized),
            Err(RunCapabilityError::InvalidFrame)
        );
        let mut raw_stream = UnixStream::connect(capability.socket())?;
        write_frame(
            &mut raw_stream,
            &RequestFrame::UpdatePrompt {
                token: String::new(),
                request_id: "update-raw-agent".to_owned(),
                agent: "sibling".to_owned(),
                session: "session-1".to_owned(),
                run: "run-1".to_owned(),
                control: "system.md".to_owned(),
                content: "different\n".to_owned(),
            },
        )?;
        assert!(matches!(
            read_json_line::<ResponseFrame>(&mut raw_stream)?,
            ResponseFrame::Error { errno, .. } if errno == "EINVAL"
        ));
        assert!(
            updates
                .lock()
                .map_err(|_error| std::io::Error::other("update capture poisoned"))?
                .is_empty()
        );
        capability.update_prompt("update-1", "system.md", "You improve yourself.\n")?;
        assert_eq!(
            capability.update_prompt("update-1", "system.md", "You improve yourself.\n"),
            Err(RunCapabilityError::Replayed)
        );
        assert_eq!(
            *updates
                .lock()
                .map_err(|_error| std::io::Error::other("update capture poisoned"))?,
            [("system.md".to_owned(), "You improve yourself.\n".to_owned())]
        );
        shutdown.store(true, Ordering::Release);
        assert_eq!(join_server(server)?, Ok(()));
        capability.cleanup()?;
        Ok(())
    }
}
