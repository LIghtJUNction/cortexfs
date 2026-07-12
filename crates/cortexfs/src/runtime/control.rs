//! Internal capability channel for one active agent run.

use crate::{PeerCredentials, peer_credentials, support::plain::open_plain_directory};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::fmt::Write as _;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write as _};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::SyncSender;
use std::time::Duration;
use std::{os::unix::net::UnixListener, os::unix::net::UnixStream};

const TOKEN_BYTES: usize = 32;
const MAX_FRAME_BYTES: u64 = 16 * 1024;
const MAX_REQUEST_IDS: usize = 64;
const MAX_REQUEST_ID_BYTES: usize = 128;

pub struct RunCapability {
    socket: PathBuf,
    token: String,
    agent: String,
    session: String,
    run: String,
    uid: u32,
    gid: u32,
    dev: u64,
    ino: u64,
    source_receipt: Option<cortexfs_runtime_client::RuntimeSourceReceipt>,
    #[cfg(test)]
    consumed: AtomicBool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunCapabilityError {
    CannotCreate,
    CannotAccept,
    CannotRead,
    CannotWrite,
    InvalidFrame,
    PeerDenied,
    TokenDenied,
    RunChanged,
    Replayed,
    RequestSetFull,
    Unsupported,
    CleanupConflict,
}

impl RunCapabilityError {
    #[must_use]
    pub const fn errno(&self) -> &'static str {
        match *self {
            Self::Replayed => "EALREADY",
            Self::RequestSetFull => "ENOSPC",
            Self::Unsupported => "ENOSYS",
            Self::PeerDenied | Self::TokenDenied => "EACCES",
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
    pub input: String,
}

/// Stable successful child creation response.
pub use cortexfs_runtime_client::CreateChildResult;

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
        let directory_fd =
            open_plain_directory(directory).map_err(|_error| RunCapabilityError::CannotCreate)?;
        let directory_metadata = directory_fd
            .metadata()
            .map_err(|_error| RunCapabilityError::CannotCreate)?;
        if !directory_metadata.is_dir()
            || directory_metadata.file_type().is_symlink()
            || directory_metadata.uid() != nix::unistd::geteuid().as_raw()
            || directory_metadata.permissions().mode() & 0o7777 != 0o711
        {
            return Err(RunCapabilityError::CannotCreate);
        }
        let token = random_token()?;
        let socket = directory.join(format!(
            "control-{}.sock",
            token.get(..24).unwrap_or(&token)
        ));
        let name = socket
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(RunCapabilityError::CannotCreate)?;
        let bind_path = crate::support::plain::proc_fd_path(&directory_fd).join(name);
        let listener =
            UnixListener::bind(&bind_path).map_err(|_error| RunCapabilityError::CannotCreate)?;
        let identity = match socket_identity(&directory_fd, name) {
            Ok(identity) => identity,
            Err(error) => {
                let _isolated = isolate_unidentified_socket(&directory_fd, name);
                return Err(error);
            }
        };
        if let Err(error) = configure_socket(
            directory,
            &directory_fd,
            (directory_metadata.dev(), directory_metadata.ino()),
            name,
            identity,
            (uid, gid),
        ) {
            let _cleanup = quarantine_socket(&directory_fd, name, identity);
            return Err(error);
        }
        Ok((
            Self {
                socket,
                token,
                agent: agent.to_owned(),
                session: session.to_owned(),
                run: run.to_owned(),
                uid,
                gid,
                dev: identity.0,
                ino: identity.1,
                source_receipt: None,
                #[cfg(test)]
                consumed: AtomicBool::new(false),
            },
            listener,
        ))
    }

    #[must_use]
    pub fn socket(&self) -> &Path {
        &self.socket
    }

    #[must_use]
    pub fn environment(&self, sandbox_socket: &Path) -> [(String, String); 2] {
        [
            (
                "CTX_CONTROL_SOCKET".to_owned(),
                sandbox_socket.display().to_string(),
            ),
            ("CTX_CONTROL_TOKEN".to_owned(), self.token.clone()),
        ]
    }

    pub fn serve_run(
        &self,
        listener: &UnixListener,
        shutdown: &AtomicBool,
        startup: &SyncSender<Result<(), RunCapabilityError>>,
        current_run: impl FnMut() -> Option<String>,
    ) -> Result<(), RunCapabilityError> {
        self.serve_run_with_handler(listener, shutdown, startup, current_run, |_request| {
            Err(RunCapabilityError::Unsupported)
        })
    }

    pub fn serve_run_with_handler(
        &self,
        listener: &UnixListener,
        shutdown: &AtomicBool,
        startup: &SyncSender<Result<(), RunCapabilityError>>,
        mut current_run: impl FnMut() -> Option<String>,
        mut create_child: impl FnMut(
            CreateChildRequest,
        ) -> Result<CreateChildResult, RunCapabilityError>,
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
        self.handle_connection(&mut stream, &mut seen, &mut current_run, &mut |_request| {
            Err(RunCapabilityError::Unsupported)
        })?;
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
    ) -> Result<String, RunCapabilityError> {
        if current_run().as_deref() != Some(self.run.as_str()) {
            return Err(RunCapabilityError::RunChanged);
        }
        stream
            .set_read_timeout(Some(control_timeout()))
            .and_then(|()| stream.set_write_timeout(Some(control_timeout())))
            .map_err(|_error| RunCapabilityError::CannotRead)?;
        let peer = peer_credentials(stream).map_err(|_error| RunCapabilityError::PeerDenied)?;
        if !peer_allowed(peer, self.uid) {
            return Err(RunCapabilityError::PeerDenied);
        }
        let frame: RequestFrame = read_json_line(stream)?;
        let (token, request_id) = match frame {
            RequestFrame::Ping {
                token,
                request_id,
                agent,
                session,
                run,
            } => {
                if agent != self.agent || session != self.session || run != self.run {
                    return Err(RunCapabilityError::InvalidFrame);
                }
                (token, request_id)
            }
            RequestFrame::CreateChild {
                token,
                request_id,
                agent,
                session,
                run,
                child,
                child_session,
                input,
            } => {
                if !constant_time_eq(token.as_bytes(), self.token.as_bytes()) {
                    return Err(RunCapabilityError::TokenDenied);
                }
                if !legal_request_id(&request_id) {
                    return Err(RunCapabilityError::InvalidFrame);
                }
                if current_run().as_deref() != Some(self.run.as_str()) {
                    return Err(RunCapabilityError::RunChanged);
                }
                reserve_request_id(stream, seen, &request_id)?;
                if agent != self.agent || session != self.session || run != self.run {
                    let _ignored = write_error_frame(stream, request_id, "EINVAL");
                    return Err(RunCapabilityError::InvalidFrame);
                }
                let result = create_child(CreateChildRequest {
                    agent,
                    session,
                    run,
                    child,
                    child_session,
                    input,
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
        };
        if !constant_time_eq(token.as_bytes(), self.token.as_bytes()) {
            return Err(RunCapabilityError::TokenDenied);
        }
        if !legal_request_id(&request_id) {
            return Err(RunCapabilityError::InvalidFrame);
        }
        if current_run().as_deref() != Some(self.run.as_str()) {
            return Err(RunCapabilityError::RunChanged);
        }
        reserve_request_id(stream, seen, &request_id)?;
        write_frame(
            stream,
            &ResponseFrame::Pong {
                request_id: request_id.clone(),
                receipt: self.source_receipt.clone(),
            },
        )?;
        Ok(request_id)
    }

    pub fn ping(&self, request_id: &str) -> Result<(), RunCapabilityError> {
        cortexfs_runtime_client::ping(
            &self.socket,
            &self.token,
            request_id,
            &self.agent,
            &self.session,
            &self.run,
        )
        .map(|_| ())
        .map_err(client_error)
    }

    pub fn create_child(
        &self,
        request_id: &str,
        child: &str,
        child_session: &str,
        input: &str,
    ) -> Result<CreateChildResult, RunCapabilityError> {
        cortexfs_runtime_client::create_child(
            &self.socket,
            &self.token,
            request_id,
            &self.agent,
            &self.session,
            &self.run,
            child,
            child_session,
            input,
        )
        .map_err(client_error)
    }

    pub fn cleanup(&self) -> Result<(), RunCapabilityError> {
        let parent_path = self
            .socket
            .parent()
            .ok_or(RunCapabilityError::CleanupConflict)?;
        let parent = open_plain_directory(parent_path)
            .map_err(|_error| RunCapabilityError::CleanupConflict)?;
        let name = self
            .socket
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or(RunCapabilityError::CleanupConflict)?;
        quarantine_socket(&parent, name, (self.dev, self.ino))
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
    request_id: &str,
    child: &str,
    child_session: &str,
    input: &str,
) -> Result<CreateChildResult, RunCapabilityError> {
    cortexfs_runtime_client::create_child_from_environment(request_id, child, child_session, input)
        .map_err(client_error)
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

fn configure_socket(
    directory_path: &Path,
    directory: &File,
    directory_identity: (u64, u64),
    name: &str,
    identity: (u64, u64),
    owner: (u32, u32),
) -> Result<(), RunCapabilityError> {
    let rebound =
        open_plain_directory(directory_path).map_err(|_error| RunCapabilityError::CannotCreate)?;
    let rebound_metadata = rebound
        .metadata()
        .map_err(|_error| RunCapabilityError::CannotCreate)?;
    if directory_identity != (rebound_metadata.dev(), rebound_metadata.ino()) {
        return Err(RunCapabilityError::CannotCreate);
    }
    require_socket_identity(directory, name, identity)?;
    nix::sys::stat::fchmodat(
        directory,
        name,
        nix::sys::stat::Mode::from_bits_truncate(0o600),
        nix::sys::stat::FchmodatFlags::NoFollowSymlink,
    )
    .map_err(|_error| RunCapabilityError::CannotCreate)?;
    require_socket_identity(directory, name, identity)?;
    nix::unistd::fchownat(
        directory,
        name,
        Some(nix::unistd::Uid::from_raw(owner.0)),
        Some(nix::unistd::Gid::from_raw(owner.1)),
        nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW,
    )
    .map_err(|_error| RunCapabilityError::CannotCreate)?;
    require_socket_identity(directory, name, identity)?;
    directory
        .sync_all()
        .map_err(|_error| RunCapabilityError::CannotCreate)
}

fn quarantine_socket(
    parent: &File,
    name: &str,
    expected: (u64, u64),
) -> Result<(), RunCapabilityError> {
    require_socket_identity(parent, name, expected)
        .map_err(|_error| RunCapabilityError::CleanupConflict)?;
    let quarantine = quarantine_name(name)?;
    nix::fcntl::renameat2(
        parent,
        name,
        parent,
        quarantine.as_str(),
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|_error| RunCapabilityError::CleanupConflict)?;
    if require_socket_identity(parent, &quarantine, expected).is_err() {
        let _ignored = nix::fcntl::renameat2(
            parent,
            quarantine.as_str(),
            parent,
            name,
            nix::fcntl::RenameFlags::RENAME_NOREPLACE,
        );
        return Err(RunCapabilityError::CleanupConflict);
    }
    nix::unistd::unlinkat(
        parent,
        quarantine.as_str(),
        nix::unistd::UnlinkatFlags::NoRemoveDir,
    )
    .map_err(|_error| RunCapabilityError::CleanupConflict)?;
    parent
        .sync_all()
        .map_err(|_error| RunCapabilityError::CleanupConflict)
}

fn isolate_unidentified_socket(parent: &File, name: &str) -> Result<(), RunCapabilityError> {
    let quarantine = quarantine_name(name)?;
    nix::fcntl::renameat2(
        parent,
        name,
        parent,
        quarantine.as_str(),
        nix::fcntl::RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|_error| RunCapabilityError::CleanupConflict)?;
    parent
        .sync_all()
        .map_err(|_error| RunCapabilityError::CleanupConflict)
}

fn quarantine_name(name: &str) -> Result<String, RunCapabilityError> {
    let suffix = random_token().map_err(|_error| RunCapabilityError::CleanupConflict)?;
    Ok(format!(
        ".{name}.rollback-{}",
        suffix.get(..16).unwrap_or(&suffix)
    ))
}

fn peer_allowed(peer: PeerCredentials, uid: u32) -> bool {
    peer.uid() == uid
}

fn control_timeout() -> Duration {
    if cfg!(test) {
        Duration::from_millis(100)
    } else {
        Duration::from_secs(5)
    }
}

fn socket_identity(parent: &File, name: &str) -> Result<(u64, u64), RunCapabilityError> {
    let stat = nix::sys::stat::fstatat(parent, name, nix::fcntl::AtFlags::AT_SYMLINK_NOFOLLOW)
        .map_err(|_error| RunCapabilityError::CannotCreate)?;
    let kind = nix::sys::stat::SFlag::from_bits_truncate(stat.st_mode);
    if !kind.contains(nix::sys::stat::SFlag::S_IFSOCK) {
        return Err(RunCapabilityError::CannotCreate);
    }
    Ok((stat.st_dev, stat.st_ino))
}

fn require_socket_identity(
    parent: &File,
    name: &str,
    expected: (u64, u64),
) -> Result<(), RunCapabilityError> {
    if socket_identity(parent, name)? == expected {
        Ok(())
    } else {
        Err(RunCapabilityError::CleanupConflict)
    }
}

fn random_token() -> Result<String, RunCapabilityError> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|_error| RunCapabilityError::CannotCreate)?;
    let mut token = String::with_capacity(TOKEN_BYTES * 2);
    for byte in bytes {
        write!(&mut token, "{byte:02x}").map_err(|_error| RunCapabilityError::CannotCreate)?;
    }
    Ok(token)
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0),
        );
    }
    difference == 0
}

fn legal_request_id(request_id: &str) -> bool {
    !request_id.is_empty()
        && request_id.len() <= MAX_REQUEST_ID_BYTES
        && request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
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

impl fmt::Display for RunCapabilityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match *self {
            Self::CannotCreate => "cannot create run capability",
            Self::CannotAccept => "cannot accept run capability connection",
            Self::CannotRead => "cannot read run capability frame",
            Self::CannotWrite => "cannot write run capability frame",
            Self::InvalidFrame => "invalid run capability frame",
            Self::PeerDenied => "run capability peer denied",
            Self::TokenDenied => "run capability token denied",
            Self::RunChanged => "run capability active run changed",
            Self::Replayed => "run capability already consumed",
            Self::RequestSetFull => "run capability request set full",
            Self::Unsupported => "run capability operation unsupported",
            Self::CleanupConflict => "run capability cleanup conflict",
        };
        f.write_str(message)
    }
}

impl fmt::Debug for RunCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("RunCapability");
        debug
            .field("socket", &self.socket)
            .field("token", &"[REDACTED]")
            .field("agent", &self.agent)
            .field("session", &self.session)
            .field("run", &self.run)
            .field("uid", &self.uid)
            .field("gid", &self.gid)
            .field("dev", &self.dev)
            .field("ino", &self.ino)
            .field("source_receipt", &self.source_receipt);
        #[cfg(test)]
        debug.field("consumed", &self.consumed.load(Ordering::Acquire));
        debug.finish()
    }
}

impl std::error::Error for RunCapabilityError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::FileTypeExt;
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
        Ok((root, Arc::new(capability), listener))
    }

    fn serve(
        capability: Arc<RunCapability>,
        listener: UnixListener,
        run: &'static str,
    ) -> thread::JoinHandle<Result<(), RunCapabilityError>> {
        thread::spawn(move || capability.serve_ping(&listener, || Some(run.to_owned())))
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
            (capability.dev, capability.ino)
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
    fn token_and_run_validation_are_strict() {
        assert!(constant_time_eq(b"same", b"same"));
        assert!(!constant_time_eq(b"same", b"diff"));
        assert!(!constant_time_eq(b"short", b"longer"));
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
    fn wrong_token_and_wrong_run_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
        let (_root, capability, listener) = fixture()?;
        let server = serve(Arc::clone(&capability), listener, "run-1");
        let frame = RequestFrame::Ping {
            token: "00".repeat(TOKEN_BYTES),
            request_id: "request-1".to_owned(),
            agent: "parent".to_owned(),
            session: "session-1".to_owned(),
            run: "run-1".to_owned(),
        };
        let mut bytes = serde_json::to_vec(&frame)?;
        bytes.push(b'\n');
        send_raw(capability.socket(), &bytes)?;
        assert_eq!(
            server
                .join()
                .map_err(|_error| std::io::Error::other("server panicked"))?,
            Err(RunCapabilityError::TokenDenied)
        );

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
        assert_eq!(
            server
                .join()
                .map_err(|_error| std::io::Error::other("server panicked"))?,
            Err(RunCapabilityError::InvalidFrame)
        );

        let (_root, capability, listener) = fixture()?;
        let server = serve(Arc::clone(&capability), listener, "run-1");
        let oversized = vec![b'x'; usize::try_from(MAX_FRAME_BYTES)? + 1];
        send_raw(capability.socket(), &oversized)?;
        assert_eq!(
            server
                .join()
                .map_err(|_error| std::io::Error::other("server panicked"))?,
            Err(RunCapabilityError::InvalidFrame)
        );
        Ok(())
    }

    #[test]
    fn missing_token_frame_run_and_changed_run_are_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        let (_root, capability, listener) = fixture()?;
        let server = serve(Arc::clone(&capability), listener, "run-1");
        send_raw(
            capability.socket(),
            br#"{"op":"ping","request_id":"request-1","agent":"parent","session":"session-1","run":"run-1"}
"#,
        )?;
        assert_eq!(
            server
                .join()
                .map_err(|_error| std::io::Error::other("server panicked"))?,
            Err(RunCapabilityError::InvalidFrame)
        );

        let (_root, capability, listener) = fixture()?;
        let server = serve(Arc::clone(&capability), listener, "run-1");
        let frame = RequestFrame::Ping {
            token: capability.token.clone(),
            request_id: "request-2".to_owned(),
            agent: "parent".to_owned(),
            session: "session-1".to_owned(),
            run: "wrong-run".to_owned(),
        };
        let mut bytes = serde_json::to_vec(&frame)?;
        bytes.push(b'\n');
        send_raw(capability.socket(), &bytes)?;
        assert_eq!(
            server
                .join()
                .map_err(|_error| std::io::Error::other("server panicked"))?,
            Err(RunCapabilityError::InvalidFrame)
        );

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
            token: capability.token.clone(),
            request_id: "request-2".to_owned(),
            agent: "parent".to_owned(),
            session: "session-1".to_owned(),
            run: "run-1".to_owned(),
        };
        let mut bytes = serde_json::to_vec(&frame)?;
        bytes.push(b'\n');
        send_raw(capability.socket(), &bytes)?;
        assert_eq!(
            server
                .join()
                .map_err(|_error| std::io::Error::other("server panicked"))?,
            Err(RunCapabilityError::RunChanged)
        );
        Ok(())
    }

    #[test]
    fn peer_and_timeouts_are_bounded() -> Result<(), Box<dyn std::error::Error>> {
        assert!(!peer_allowed(PeerCredentials::new(None, 1001, 1002), 1000));
        assert!(!peer_allowed(PeerCredentials::new(None, 0, 0), 1000));
        assert!(peer_allowed(PeerCredentials::new(None, 1000, 1002), 1000));
        let (_root, capability, listener) = fixture()?;
        assert_eq!(
            capability.serve_ping(&listener, || Some("run-1".to_owned())),
            Err(RunCapabilityError::CannotAccept)
        );

        let (_root, capability, listener) = fixture()?;
        let server = serve(Arc::clone(&capability), listener, "run-1");
        let _idle = UnixStream::connect(capability.socket())?;
        assert_eq!(
            server
                .join()
                .map_err(|_error| std::io::Error::other("server panicked"))?,
            Err(RunCapabilityError::CannotRead)
        );
        Ok(())
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
        assert_eq!(
            server
                .join()
                .map_err(|_error| std::io::Error::other("server panicked"))?,
            Ok(())
        );
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
                token: capability.token.clone(),
                request_id: "create-1".to_owned(),
                agent: "parent".to_owned(),
                session: "session-1".to_owned(),
                run: "run-1".to_owned(),
                child: "child".to_owned(),
                child_session: "child-session".to_owned(),
                input: "work".to_owned(),
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
            format!(
                "{{\"op\":\"agent.create\",\"token\":\"{}\",\"request_id\":\"create-2\",\"uid\":0}}\n",
                capability.token
            )
            .as_bytes(),
        )?;
        assert_eq!(capability.ping("request-after-invalid"), Ok(()));
        shutdown.store(true, Ordering::Release);
        assert_eq!(
            server
                .join()
                .map_err(|_error| std::io::Error::other("server panicked"))?,
            Ok(())
        );
        capability.cleanup()?;
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
                    Ok(CreateChildResult {
                        child: request.child,
                        child_session: request.child_session,
                        pid: 42,
                    })
                },
            )
        });
        capability.ping("startup-run-1")?;
        assert_eq!(startup_receiver.recv()?, Ok(()));
        assert_eq!(
            capability.create_child("create-1", "child-a", "session-a", "work")?,
            CreateChildResult {
                child: "child-a".to_owned(),
                child_session: "session-a".to_owned(),
                pid: 42,
            }
        );
        assert_eq!(
            capability.create_child("create-1", "child-a", "session-a", "work"),
            Err(RunCapabilityError::Replayed)
        );
        assert_eq!(
            capability
                .create_child("create-2", "child-b", "session-b", "other")?
                .pid,
            42
        );
        assert_eq!(calls.load(Ordering::Acquire), 2);
        shutdown.store(true, Ordering::Release);
        assert_eq!(
            server
                .join()
                .map_err(|_error| std::io::Error::other("server panicked"))?,
            Ok(())
        );
        capability.cleanup()?;
        Ok(())
    }
}
