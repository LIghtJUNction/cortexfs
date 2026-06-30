/// Error while reading Unix socket peer credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerCredentialError {
    /// Kernel peer credential lookup failed.
    CannotRead,
}

/// Durable socket request recording error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketSessionRecordError {
    /// `resume` and `ping` do not mutate durable session files.
    UnsupportedRequest,
    /// Temp sessions are process-local and need not have durable files.
    TempSessionNotDurable,
    /// The request names a different session than the target directory.
    SessionMismatch,
    /// A required durable session file is missing.
    MissingSessionFile(&'static str),
    /// A supplied stable field is malformed.
    InvalidField(&'static str),
    /// Session files could not be updated.
    CannotRecord,
}

/// Socket runtime request handling error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SocketRuntimeError {
    /// Request frame is not valid `CortexFS` socket JSONL.
    Request(SocketRequestError),
    /// Durable session layout could not be ensured.
    SessionLayout(DurableSessionLayoutError),
    /// Durable send plus index update failed.
    IndexedRecord(IndexedSocketSessionRecordError),
    /// Durable non-send request mutation failed.
    Record(SocketSessionRecordError),
    /// Session name or index state is invalid.
    InvalidSessionName,
    /// Session event stream could not be read.
    CannotReadEvents,
    /// Kernel peer credential lookup failed.
    PeerCredential(PeerCredentialError),
    /// Connected peer does not match the required socket peer policy.
    PeerDenied,
    /// Request frame could not be read from a Unix socket stream.
    CannotReadFrame,
    /// Response frame could not be written to a Unix socket stream.
    CannotWriteResponse,
    /// Unix socket listener could not accept a connection.
    CannotAcceptConnection,
    /// Agent executable path is not an absolute regular file.
    InvalidAgentExecutable,
    /// Agent executable could not be run.
    CannotRunAgent,
    /// Agent executable returned invalid canonical event JSONL.
    InvalidAgentOutput,
}

/// JSONL lines durably recorded for one socket request.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SocketSessionRecord {
    messages: Vec<String>,
    events: Vec<String>,
}

/// Canonical JSONL response frames produced for one socket request.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SocketRuntimeResponse {
    frames: Vec<String>,
}

/// Error while recording a socket send and updating the session index.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IndexedSocketSessionRecordError {
    /// Durable session files could not be recorded.
    Session(SocketSessionRecordError),
    /// Reserved session index files could not be updated.
    Index(SessionIndexUpdateError),
}

/// Linux peer credentials for a connected Unix socket.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerCredentials {
    pid: Option<i32>,
    uid: u32,
    gid: u32,
}

/// Required peer identity for a socket operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SocketPeerPolicy {
    uid: Option<u32>,
    gid: Option<u32>,
}

/// Runtime inputs for dispatching socket `send` frames to an agent executable.
#[derive(Clone, Copy, Debug)]
pub struct AgentExecutableSocketRuntime<'a> {
    /// FUSE ABI root used by executable agents for object calls.
    pub ctx_root: &'a Path,
    /// Backing source root used for object control files.
    pub source_root: &'a Path,
    /// Runtime Linux uid/gid/groups applied before executing the agent.
    pub identity: &'a AgentUnixIdentity,
    /// Runtime environment derived from `agent/<name>.d/env` plus reserved `CTX_*` values.
    pub env: &'a [(String, String)],
    /// Durable session root for the selected agent.
    pub session_root: &'a Path,
    /// Default chroot cwd when a request does not provide one.
    pub default_cwd: &'a str,
    /// Selected model object name from `agent/<name>.d/model`.
    pub model: Option<&'a str>,
    /// Whether the agent policy permits network egress.
    pub network_allowed: bool,
    /// Agent object name.
    pub agent_name: &'a str,
    /// ABI executable object to invoke for `send`.
    pub agent_executable: &'a Path,
    /// Process isolation mode for the executable agent.
    pub execution: AgentExecutableSocketExecution<'a>,
}

/// Process isolation mode for socket-activated executable agents.
#[derive(Clone, Copy, Debug)]
pub enum AgentExecutableSocketExecution<'a> {
    /// Execute the agent object directly. This is only for unit tests and
    /// tightly controlled development helpers.
    Direct,
    /// Execute the agent object inside the `CortexFS` `bwrap` sandbox.
    Bwrap {
        /// Bubblewrap executable.
        program: &'a Path,
        /// Agent mount table derived from `agent/<name>.d/mount`.
        mount_table: &'a MountTable,
    },
}

impl SocketRuntimeError {
    /// Returns a stable errno name for this runtime failure.
    #[must_use]
    pub const fn errno(&self) -> &'static str {
        match *self {
            Self::Request(ref error) => error.errno(),
            Self::SessionLayout(error) => error.errno(),
            Self::IndexedRecord(error) => error.errno(),
            Self::Record(error) => error.errno(),
            Self::InvalidSessionName => "EINVAL",
            Self::PeerDenied => "EACCES",
            Self::CannotReadEvents
            | Self::PeerCredential(_)
            | Self::CannotReadFrame
            | Self::CannotWriteResponse
            | Self::CannotAcceptConnection
            | Self::CannotRunAgent
            | Self::InvalidAgentOutput => "EIO",
            Self::InvalidAgentExecutable => "ENOENT",
        }
    }
}

impl SocketSessionRecordError {
    /// Returns a stable errno name for this recording failure.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::UnsupportedRequest | Self::SessionMismatch | Self::InvalidField(_) => "EINVAL",
            Self::TempSessionNotDurable | Self::MissingSessionFile(_) => "ENOENT",
            Self::CannotRecord => "EIO",
        }
    }
}

impl IndexedSocketSessionRecordError {
    /// Returns a stable errno name for this indexed recording failure.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::Session(error) => error.errno(),
            Self::Index(error) => error.errno(),
        }
    }
}

impl SocketSessionRecord {
    /// Creates a record from message and event JSONL lines.
    #[must_use]
    pub const fn new(messages: Vec<String>, events: Vec<String>) -> Self {
        Self { messages, events }
    }

    /// Returns appended `messages.jsonl` lines.
    #[must_use]
    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    /// Returns appended `events.jsonl` lines.
    #[must_use]
    pub fn events(&self) -> &[String] {
        &self.events
    }
}

impl SocketRuntimeResponse {
    /// Creates a response from complete JSON object frames, without trailing newlines.
    #[must_use]
    pub const fn new(frames: Vec<String>) -> Self {
        Self { frames }
    }

    /// Returns response frames without trailing newlines.
    #[must_use]
    pub fn frames(&self) -> &[String] {
        &self.frames
    }

    /// Returns response frames as JSONL.
    #[must_use]
    pub fn jsonl(&self) -> String {
        if self.frames.is_empty() {
            String::new()
        } else {
            format!("{}\n", self.frames.join("\n"))
        }
    }
}

impl PeerCredentials {
    /// Creates a peer credential record.
    #[must_use]
    pub const fn new(pid: Option<i32>, uid: u32, gid: u32) -> Self {
        Self { pid, uid, gid }
    }

    /// Returns the peer process id when the kernel reports it.
    #[must_use]
    pub const fn pid(self) -> Option<i32> {
        self.pid
    }

    /// Returns the peer user id.
    #[must_use]
    pub const fn uid(self) -> u32 {
        self.uid
    }

    /// Returns the peer group id.
    #[must_use]
    pub const fn gid(self) -> u32 {
        self.gid
    }
}

impl SocketPeerPolicy {
    /// Requires a specific peer uid.
    #[must_use]
    pub const fn uid(uid: u32) -> Self {
        Self {
            uid: Some(uid),
            gid: None,
        }
    }

    /// Requires a specific peer gid.
    #[must_use]
    pub const fn gid(gid: u32) -> Self {
        Self {
            uid: None,
            gid: Some(gid),
        }
    }

    /// Requires both peer uid and gid.
    #[must_use]
    pub const fn uid_gid(uid: u32, gid: u32) -> Self {
        Self {
            uid: Some(uid),
            gid: Some(gid),
        }
    }

    /// Returns whether the peer credentials satisfy this policy.
    #[must_use]
    pub fn allows(self, peer: PeerCredentials) -> bool {
        self.uid.is_none_or(|uid| peer.uid() == uid) && self.gid.is_none_or(|gid| peer.gid() == gid)
    }
}
