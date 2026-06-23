//! `CortexFS` Agent OS ABI design core.
//!
//! The old CLI, daemon, provider registry, and FUSE projection were removed
//! before the Agent OS rewrite. This crate intentionally exposes only stable
//! ABI names while the implementation is redesigned around Rig.

use std::collections::HashSet;
use std::fs;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt, symlink};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use nix::sys::socket::{getsockopt, sockopt};
use serde::Deserialize;
use serde_json::Value;

mod abi_constants;
mod abi_path;
mod agent_control;
mod context_pack;
mod model;
mod mount_table;
mod policy;
mod session_index;
mod shared_queue;
mod socket_request;
mod stream;
mod tool_path;
mod tool_schema;

pub use abi_constants::{
    AGENT_CONTROL_FILES, CHILD_RESULT_REQUIRED_DIRS, CHILD_RESULT_REQUIRED_FILES,
    CONTEXT_REQUIRED_DIRS, CONTEXT_REQUIRED_FILES, CORTEXFS_OBJECT_RUNNER, CTX_ROOT, EXEC_OBJECTS,
    FORBIDDEN_MODEL_CAPABILITIES, FUSE_V1_ROOT_INODE, MAX_FUSE_V1_SMALL_WRITE_BYTES,
    MAX_OBJECT_NAME_LEN, MAX_SOCKET_FRAME_BYTES, MODEL_CONTROL_FILES, ROOT_ENTRIES,
    SESSION_REQUIRED_FILES, SHARED_QUEUE_REQUIRED_DIRS, STABLE_MODEL_CAPABILITIES,
    TOOL_CONTROL_FILES,
};
use abi_constants::{
    DEBUG_ECHO_MODEL, DEBUG_ECHO_NAME, DEBUG_ECHO_PROVIDER, DEFAULT_MODEL_ALIAS,
    DEFAULT_MODEL_ALIAS_TARGET, HELPER_MODEL_ALIAS, SYSTEM_PROVIDER_CONFIG_DIR,
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

/// File kind exposed by the v1 FUSE projection layer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FuseV1FileType {
    /// Directory entry.
    Directory,
    /// Regular file.
    Regular,
    /// Symbolic link.
    Symlink,
    /// Unix domain socket.
    Socket,
    /// Other filesystem object.
    Other,
}

/// Minimal attributes needed by a FUSE adapter for v1 ABI paths.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuseV1Attr {
    abi_path: String,
    file_type: FuseV1FileType,
    size: u64,
    mode: u32,
    uid: u32,
    gid: u32,
}

/// Directory entry returned by the v1 FUSE projection.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuseV1DirEntry {
    name: String,
    file_type: FuseV1FileType,
}

/// Path/inode pair used by a FUSE adapter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuseV1Node {
    inode: u64,
    abi_path: String,
    attr: FuseV1Attr,
}

/// Error returned by the local v1 FUSE projection helpers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FuseV1Error {
    /// ABI path escaped the `/ctx` root or used invalid syntax.
    InvalidPath,
    /// Path does not exist.
    NotFound,
    /// Operation requires a directory.
    NotDirectory,
    /// Operation requires a readable regular file or symlink target.
    NotFile,
    /// Writes through this projection are limited to ABI control files.
    NotControlFile,
    /// Control-file write did not start at offset zero.
    InvalidOffset,
    /// Control-file payload was not valid UTF-8 text.
    InvalidContent,
    /// Write exceeds the v1 small-control-file limit.
    TooLarge,
    /// Underlying filesystem operation failed.
    Io,
}

/// Local v1 FUSE projection backend over an existing `/ctx`-shaped tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FuseV1Projection {
    root: PathBuf,
    provider_config_dir: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct VirtualExecObject {
    class: ObjectClass,
    name: String,
    control_dir: PathBuf,
}

#[derive(Clone, Debug, Deserialize)]
struct ProviderConfig {
    base_url: String,
    default_model: Option<String>,
    #[serde(default)]
    models: Vec<String>,
    #[serde(default = "default_provider_enabled")]
    enabled: bool,
    #[serde(default)]
    formats: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProjectedProviderModel {
    provider: String,
    model: String,
    base_url: String,
    driver: String,
    cap: String,
}

/// Error while reading Unix socket peer credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerCredentialError {
    /// Kernel peer credential lookup failed.
    CannotRead,
}

/// Session layout validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionLayoutIssue {
    /// Required file is missing.
    MissingFile(String),
    /// Required directory is missing.
    MissingDirectory(String),
    /// Path exists but is not a regular file.
    NotFile(String),
    /// Path exists but is not a directory.
    NotDirectory(String),
    /// Required session control file has a value outside the v1 stable set.
    InvalidFileValue { path: String, value: String },
}

/// Stable session control file kind with fixed v1 value syntax.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionControlKind {
    /// `state`: active, idle, done, error, or cancelled.
    State,
    /// `cwd`: absolute path inside the agent chroot.
    Cwd,
    /// `meta.json`: JSON object with optional stable fields.
    MetaJson,
}

impl SessionControlKind {
    /// Parses a durable session control file name.
    #[must_use]
    pub fn parse(file_name: &str) -> Option<Self> {
        match file_name {
            "state" => Some(Self::State),
            "cwd" => Some(Self::Cwd),
            "meta.json" => Some(Self::MetaJson),
            _ => None,
        }
    }
}

/// Session control-file validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionControlIssue {
    /// A required single value is empty.
    EmptyValue,
    /// A single-value control file contains more than one line.
    MultipleValues { line: usize },
    /// Fixed vocabulary, path, or field value is malformed.
    InvalidValue { line: usize, value: String },
    /// `meta.json` is not valid JSON.
    InvalidJson,
    /// `meta.json` is valid JSON but not an object.
    NotObject,
}

/// Result of inspecting a fixed-format session control file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionControlReport {
    issues: Vec<SessionControlIssue>,
}

impl SessionLayoutIssue {
    /// Returns a stable short description of the issue kind.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match *self {
            Self::MissingFile(_) => "missing file",
            Self::MissingDirectory(_) => "missing directory",
            Self::NotFile(_) => "not file",
            Self::NotDirectory(_) => "not directory",
            Self::InvalidFileValue { .. } => "invalid file value",
        }
    }

    /// Returns the relative session path associated with the issue.
    #[must_use]
    pub fn path(&self) -> &str {
        match *self {
            Self::MissingFile(ref path)
            | Self::MissingDirectory(ref path)
            | Self::NotFile(ref path)
            | Self::NotDirectory(ref path)
            | Self::InvalidFileValue { ref path, .. } => path,
        }
    }

    /// Returns the invalid value, when the issue records one.
    #[must_use]
    pub fn value(&self) -> Option<&str> {
        match *self {
            Self::InvalidFileValue { ref value, .. } => Some(value),
            Self::MissingFile(_)
            | Self::MissingDirectory(_)
            | Self::NotFile(_)
            | Self::NotDirectory(_) => None,
        }
    }
}

/// Result of inspecting a durable session directory.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionLayoutReport {
    issues: Vec<SessionLayoutIssue>,
}

impl SessionLayoutReport {
    /// Creates a report with collected layout issues.
    #[must_use]
    pub const fn new(issues: Vec<SessionLayoutIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when the session satisfies the v1 layout.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected layout issues.
    #[must_use]
    pub fn issues(&self) -> &[SessionLayoutIssue] {
        &self.issues
    }
}

impl SessionControlReport {
    /// Creates a report with collected session control issues.
    #[must_use]
    pub const fn new(issues: Vec<SessionControlIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when the control file satisfies the fixed v1 syntax.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected session control issues.
    #[must_use]
    pub fn issues(&self) -> &[SessionControlIssue] {
        &self.issues
    }
}

/// Object layout validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectLayoutIssue {
    /// Executable entry is missing.
    MissingExecutable(String),
    /// Executable entry exists but is not executable.
    NotExecutable(String),
    /// Control directory is missing.
    MissingControlDirectory(String),
    /// Control path exists but is not a directory.
    NotControlDirectory(String),
    /// Required control file is missing.
    MissingControlFile(String),
    /// Required control path exists but is not a regular file.
    NotControlFile(String),
    /// Required socket is missing.
    MissingSocket(String),
    /// Socket path exists but is not a Unix socket.
    NotSocket(String),
    /// Control file has a value outside the v1 stable set.
    InvalidControlValue { path: String, value: String },
}

impl ObjectLayoutIssue {
    /// Returns a stable short description of the issue kind.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match *self {
            Self::MissingExecutable(_) => "missing executable",
            Self::NotExecutable(_) => "not executable",
            Self::MissingControlDirectory(_) => "missing control directory",
            Self::NotControlDirectory(_) => "not control directory",
            Self::MissingControlFile(_) => "missing control file",
            Self::NotControlFile(_) => "not control file",
            Self::MissingSocket(_) => "missing socket",
            Self::NotSocket(_) => "not socket",
            Self::InvalidControlValue { .. } => "invalid control value",
        }
    }

    /// Returns the relative ABI path associated with the issue.
    #[must_use]
    pub fn path(&self) -> &str {
        match *self {
            Self::MissingExecutable(ref path)
            | Self::NotExecutable(ref path)
            | Self::MissingControlDirectory(ref path)
            | Self::NotControlDirectory(ref path)
            | Self::MissingControlFile(ref path)
            | Self::NotControlFile(ref path)
            | Self::MissingSocket(ref path)
            | Self::NotSocket(ref path)
            | Self::InvalidControlValue { ref path, .. } => path,
        }
    }

    /// Returns the invalid value, when the issue records one.
    #[must_use]
    pub fn value(&self) -> Option<&str> {
        match *self {
            Self::InvalidControlValue { ref value, .. } => Some(value),
            Self::MissingExecutable(_)
            | Self::NotExecutable(_)
            | Self::MissingControlDirectory(_)
            | Self::NotControlDirectory(_)
            | Self::MissingControlFile(_)
            | Self::NotControlFile(_)
            | Self::MissingSocket(_)
            | Self::NotSocket(_) => None,
        }
    }
}

/// Result of inspecting a model, agent, or tool object triple.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ObjectLayoutReport {
    issues: Vec<ObjectLayoutIssue>,
}

/// Result of installing an executable object wrapper and control directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectBootstrap {
    executable: PathBuf,
    control_dir: PathBuf,
}

/// Result of materializing the documented v1 reference tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceTreeBootstrap {
    root: PathBuf,
}

/// Error while installing a v1 executable object wrapper.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectBootstrapError {
    /// Object name is not a valid v1 path component.
    InvalidObjectName,
    /// Wrapper target command is empty or contains an unsafe control byte.
    InvalidWrapperTarget,
    /// Override names a file outside the stable control file set.
    InvalidControlFile,
    /// Override value does not satisfy stable syntax for that control file.
    InvalidControlValue,
    /// Object directories could not be created.
    CannotCreate,
    /// Executable or control files could not be written.
    CannotRecord,
    /// Executable permissions could not be set.
    CannotChmod,
}

/// Error while resolving a provider API key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ApiKeyResolutionError {
    /// Environment variable, service, or account name is invalid.
    InvalidName,
    /// System keychain command failed in an unexpected way.
    KeychainUnavailable,
}

/// Linux peer credentials for a connected Unix socket.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerCredentials {
    pid: Option<i32>,
    uid: u32,
    gid: u32,
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

/// Durable session layout creation error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DurableSessionLayoutError {
    /// Session name is not a valid v1 object name.
    InvalidSessionName,
    /// Initial cwd is not an absolute chroot path.
    InvalidCwd,
    /// Optional model name is not a valid v1 object name.
    InvalidModelName,
    /// Temp sessions are process-local and are not durable.
    TempSessionNotDurable,
    /// Required files or directories could not be created.
    CannotCreate,
}

/// Error while materializing the documented v1 reference tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceTreeError {
    /// A root directory, subdirectory, or ordinary file could not be created.
    CannotCreate,
    /// A stable executable object could not be bootstrapped.
    Object(ObjectBootstrapError),
    /// A durable session layout could not be ensured.
    Session(DurableSessionLayoutError),
    /// A parent-owned child result channel could not be recorded.
    Child(ChildContextRecordError),
    /// A documented symlink could not be created or conflicts with an existing path.
    CannotLink,
    /// A documented socket path could not be created or conflicts with an existing path.
    CannotSocket,
    /// A deprecated reference-tree placeholder could not be removed.
    CannotRemove,
    /// A deprecated reference-tree alias could not be removed.
    CannotUnlink,
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
    /// Agent executable could not be run.
    CannotRunAgent,
    /// Agent executable returned invalid canonical event JSONL.
    InvalidAgentOutput,
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
        }
    }
}

impl DurableSessionLayoutError {
    /// Returns a stable errno name for this layout creation failure.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidSessionName | Self::InvalidCwd | Self::InvalidModelName => "EINVAL",
            Self::TempSessionNotDurable => "ENOENT",
            Self::CannotCreate => "EIO",
        }
    }
}

impl ReferenceTreeError {
    /// Returns a stable errno name for this reference-tree bootstrap failure.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::CannotCreate
            | Self::Object(
                ObjectBootstrapError::CannotCreate
                | ObjectBootstrapError::CannotRecord
                | ObjectBootstrapError::CannotChmod,
            )
            | Self::Session(DurableSessionLayoutError::CannotCreate)
            | Self::Child(ChildContextRecordError::CannotRecord)
            | Self::CannotLink
            | Self::CannotSocket
            | Self::CannotRemove
            | Self::CannotUnlink => "EIO",
            Self::Object(
                ObjectBootstrapError::InvalidObjectName
                | ObjectBootstrapError::InvalidWrapperTarget
                | ObjectBootstrapError::InvalidControlFile
                | ObjectBootstrapError::InvalidControlValue,
            )
            | Self::Session(
                DurableSessionLayoutError::InvalidSessionName
                | DurableSessionLayoutError::InvalidCwd
                | DurableSessionLayoutError::InvalidModelName,
            )
            | Self::Child(
                ChildContextRecordError::InvalidChildName
                | ChildContextRecordError::InvalidAgentName
                | ChildContextRecordError::InvalidSessionName
                | ChildContextRecordError::InvalidStatus
                | ChildContextRecordError::InvalidText
                | ChildContextRecordError::InvalidRefs,
            ) => "EINVAL",
            Self::Session(DurableSessionLayoutError::TempSessionNotDurable)
            | Self::Child(ChildContextRecordError::MissingParentSession) => "ENOENT",
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
    /// Durable session root for the selected agent.
    pub session_root: &'a Path,
    /// Default chroot cwd when a request does not provide one.
    pub default_cwd: &'a str,
    /// Selected model object name from `agent/<name>.d/model`.
    pub model: Option<&'a str>,
    /// Agent object name.
    pub agent_name: &'a str,
    /// ABI executable object to invoke for `send`.
    pub agent_executable: &'a Path,
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

/// Runtime Unix identity used for Linux permission checks.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentUnixIdentity {
    uid: u32,
    gid: u32,
    groups: Vec<u32>,
}

/// Derived launch/view state for one `agent/<name>.d/` control directory.
///
/// This is a pure filesystem ABI derivation. It does not start a process,
/// create namespaces, execute tools, or interpret MCP/skill/prompt formats.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentRuntimeView {
    agent_name: String,
    control_dir: PathBuf,
    ctx_root: PathBuf,
    ctx_home: PathBuf,
    home: PathBuf,
    owner: u32,
    identity: AgentUnixIdentity,
    label: String,
    policy_subject: String,
    iso: String,
    parent: Option<String>,
    lifecycle: ChildLifecycle,
    root: PathBuf,
    cwd: PathBuf,
    env: Vec<(String, String)>,
    tool_path: ToolPath,
    mount_table: MountTable,
    model: String,
    policy: PolicyV0,
}

/// Error while deriving an agent runtime view from `agent/<name>.d/*`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentRuntimeViewError {
    /// Agent name is not a valid object name.
    InvalidAgentName,
    /// `agent/<name>.d/` is missing or is not a directory.
    MissingControlDirectory,
    /// A required control file is missing.
    MissingControlFile(String),
    /// A control file could not be read.
    CannotReadControl(String),
    /// A control file has malformed v1 content.
    InvalidControlFile(String),
}

impl AgentUnixIdentity {
    /// Creates an identity from uid, primary gid, and supplementary groups.
    #[must_use]
    pub fn new(uid: u32, gid: u32, groups: impl IntoIterator<Item = u32>) -> Self {
        Self {
            uid,
            gid,
            groups: groups.into_iter().collect(),
        }
    }

    /// Returns the runtime uid.
    #[must_use]
    pub const fn uid(&self) -> u32 {
        self.uid
    }

    /// Returns the runtime primary gid.
    #[must_use]
    pub const fn gid(&self) -> u32 {
        self.gid
    }

    /// Returns supplementary groups.
    #[must_use]
    pub fn groups(&self) -> &[u32] {
        &self.groups
    }

    fn is_in_group(&self, gid: u32) -> bool {
        self.gid == gid || self.groups.contains(&gid)
    }
}

impl AgentRuntimeView {
    /// Returns the agent object name.
    #[must_use]
    pub fn agent_name(&self) -> &str {
        &self.agent_name
    }

    /// Returns the control directory that produced this view.
    #[must_use]
    pub fn control_dir(&self) -> &Path {
        &self.control_dir
    }

    /// Returns `CTX_ROOT`.
    #[must_use]
    pub fn ctx_root(&self) -> &Path {
        &self.ctx_root
    }

    /// Returns `CTX_HOME`.
    #[must_use]
    pub fn ctx_home(&self) -> &Path {
        &self.ctx_home
    }

    /// Returns the agent `HOME`.
    #[must_use]
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// Returns the owning Linux uid from `owner`.
    #[must_use]
    pub const fn owner(&self) -> u32 {
        self.owner
    }

    /// Returns the runtime Linux identity from `uid/gid/groups`.
    #[must_use]
    pub const fn identity(&self) -> &AgentUnixIdentity {
        &self.identity
    }

    /// Returns the full label control value.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }

    /// Returns the v0 policy subject used for effective-authority checks.
    #[must_use]
    pub fn policy_subject(&self) -> &str {
        &self.policy_subject
    }

    /// Returns the isolation profile.
    #[must_use]
    pub fn iso(&self) -> &str {
        &self.iso
    }

    /// Returns the optional parent reference.
    #[must_use]
    pub fn parent(&self) -> Option<&str> {
        self.parent.as_deref()
    }

    /// Returns the v1 lifecycle value.
    #[must_use]
    pub const fn lifecycle(&self) -> ChildLifecycle {
        self.lifecycle
    }

    /// Returns the chroot root from `root`.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns the startup cwd inside the chroot.
    #[must_use]
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Returns the computed environment in process insertion order.
    #[must_use]
    pub fn env(&self) -> &[(String, String)] {
        &self.env
    }

    /// Returns the derived `CTX_PATH` tool lookup path.
    #[must_use]
    pub const fn tool_path(&self) -> &ToolPath {
        &self.tool_path
    }

    /// Returns the parsed mount table.
    #[must_use]
    pub const fn mount_table(&self) -> &MountTable {
        &self.mount_table
    }

    /// Returns the selected model object name.
    #[must_use]
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Returns the parsed v0 policy.
    #[must_use]
    pub const fn policy(&self) -> &PolicyV0 {
        &self.policy
    }
}

impl AgentRuntimeViewError {
    /// Returns a stable errno name for this derivation failure.
    #[must_use]
    pub const fn errno(&self) -> &'static str {
        match self {
            &Self::InvalidAgentName | &Self::InvalidControlFile(_) => "EINVAL",
            &Self::MissingControlDirectory | &Self::MissingControlFile(_) => "ENOENT",
            &Self::CannotReadControl(_) => "EIO",
        }
    }
}

/// Effective-authority refusal reason for tool execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolExecutionDenial {
    /// Tool name is not a valid v1 object name.
    InvalidToolName,
    /// No executable tool was found through `CTX_PATH`.
    ToolNotFound,
    /// A `CTX_PATH` directory could not be read.
    CannotReadToolPath,
    /// Tool metadata could not be read.
    CannotInspectTool,
    /// Linux uid/gid/groups/mode bits refuse execution.
    LinuxPermission,
    /// No mount entry exposes the selected tool path in the agent view.
    NotMounted,
    /// The selected mount is `noexec`.
    NoExecMount,
    /// Agent policy does not allow `tool:<name> execute`.
    AgentPolicy,
    /// Tool policy does not allow `tool:<name> execute`.
    ToolPolicy,
    /// Model principals may emit tool-call syntax but must not execute tools.
    ModelCannotExecute,
}

impl ToolExecutionDenial {
    /// Returns a stable errno name for this denial.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidToolName => "EINVAL",
            Self::ToolNotFound => "ENOENT",
            Self::CannotReadToolPath | Self::CannotInspectTool => "EIO",
            Self::LinuxPermission
            | Self::NotMounted
            | Self::NoExecMount
            | Self::AgentPolicy
            | Self::ToolPolicy
            | Self::ModelCannotExecute => "EACCES",
        }
    }
}

/// Stable principal class requesting tool execution.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolExecutionPrincipal {
    /// Policy-bound agent orchestrator.
    Agent,
    /// Pure inference model endpoint.
    Model,
}

/// Positive tool execution authority decision.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolExecutionGrant {
    hit: ToolHit,
}

impl ToolExecutionGrant {
    /// Creates a grant for a concrete `CTX_PATH` hit.
    #[must_use]
    pub const fn new(hit: ToolHit) -> Self {
        Self { hit }
    }

    /// Returns the executable tool selected by left-to-right `CTX_PATH`.
    #[must_use]
    pub const fn hit(&self) -> &ToolHit {
        &self.hit
    }
}

/// Inputs that define an agent's effective authority for a tool execution.
#[derive(Clone, Copy, Debug)]
pub struct ToolExecutionAuthority<'a> {
    principal: ToolExecutionPrincipal,
    identity: &'a AgentUnixIdentity,
    mount_table: &'a MountTable,
    agent_subject: &'a str,
    agent_policy: &'a PolicyV0,
    tool_policy: &'a PolicyV0,
}

impl<'a> ToolExecutionAuthority<'a> {
    /// Creates an authority context for one tool execution decision.
    #[must_use]
    pub const fn new(
        identity: &'a AgentUnixIdentity,
        mount_table: &'a MountTable,
        agent_subject: &'a str,
        agent_policy: &'a PolicyV0,
        tool_policy: &'a PolicyV0,
    ) -> Self {
        Self {
            principal: ToolExecutionPrincipal::Agent,
            identity,
            mount_table,
            agent_subject,
            agent_policy,
            tool_policy,
        }
    }

    /// Creates an authority context for a model-originated tool execution
    /// attempt. This always denies at the `CortexFS` boundary.
    #[must_use]
    pub const fn model(
        identity: &'a AgentUnixIdentity,
        mount_table: &'a MountTable,
        model_subject: &'a str,
        agent_policy: &'a PolicyV0,
        tool_policy: &'a PolicyV0,
    ) -> Self {
        Self {
            principal: ToolExecutionPrincipal::Model,
            identity,
            mount_table,
            agent_subject: model_subject,
            agent_policy,
            tool_policy,
        }
    }
}

/// Shared-space operation kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedAccess {
    /// Read from a shared space.
    Read,
    /// Write to a shared space.
    Write,
}

impl SharedAccess {
    fn policy_permission(self) -> PolicyPermission {
        match self {
            Self::Read => PolicyPermission::Read,
            Self::Write => PolicyPermission::Write,
        }
    }
}

/// Durable session operation kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionAccess {
    /// Read session history or derived context.
    Read,
    /// Write session history or derived context.
    Write,
    /// Resume a session through the socket protocol.
    Resume,
}

impl SessionAccess {
    fn policy_permission(self) -> PolicyPermission {
        match self {
            Self::Read => PolicyPermission::Read,
            Self::Write => PolicyPermission::Write,
            Self::Resume => PolicyPermission::Resume,
        }
    }

    fn shared_policy_permission(self) -> PolicyPermission {
        match self {
            Self::Read | Self::Resume => PolicyPermission::Read,
            Self::Write => PolicyPermission::Write,
        }
    }
}

/// Effective-authority refusal reason for shared-space access.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedAccessDenial {
    /// Shared-space name is not a valid v1 object name.
    InvalidSharedName,
    /// Path is not a stable shared-space path for the named space.
    WrongSharedPath,
    /// Shared path metadata could not be read.
    CannotInspectPath,
    /// No mount entry exposes the selected shared path in the agent view.
    NotMounted,
    /// A write was requested through a read-only mount.
    ReadOnlyMount,
    /// Linux uid/gid/groups/mode bits refuse access.
    LinuxPermission,
    /// Agent policy does not allow the requested shared-space access.
    Policy,
}

impl SharedAccessDenial {
    /// Returns a stable errno name for this denial.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidSharedName | Self::WrongSharedPath => "EINVAL",
            Self::CannotInspectPath => "EIO",
            Self::NotMounted | Self::ReadOnlyMount | Self::LinuxPermission | Self::Policy => {
                "EACCES"
            }
        }
    }
}

/// Effective-authority refusal reason for durable session access.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionAccessDenial {
    /// Path is not a stable private or shared session path.
    InvalidSessionPath,
    /// Session path metadata could not be read.
    CannotInspectPath,
    /// No mount entry exposes the selected session path in the agent view.
    NotMounted,
    /// A write was requested through a read-only mount.
    ReadOnlyMount,
    /// Linux uid/gid/groups/mode bits or private home uid refuse access.
    LinuxPermission,
    /// Shared-space policy does not allow the requested access.
    SharedPolicy,
    /// Session policy does not allow the requested access.
    SessionPolicy,
}

impl SessionAccessDenial {
    /// Returns a stable errno name for this denial.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidSessionPath => "EINVAL",
            Self::CannotInspectPath => "EIO",
            Self::NotMounted
            | Self::ReadOnlyMount
            | Self::LinuxPermission
            | Self::SharedPolicy
            | Self::SessionPolicy => "EACCES",
        }
    }
}

/// Inputs that define an agent's effective authority for shared-space access.
#[derive(Clone, Copy, Debug)]
pub struct SharedAccessAuthority<'a> {
    identity: &'a AgentUnixIdentity,
    mount_table: &'a MountTable,
    agent_subject: &'a str,
    policy: &'a PolicyV0,
}

impl<'a> SharedAccessAuthority<'a> {
    /// Creates an authority context for one shared-space access decision.
    #[must_use]
    pub const fn new(
        identity: &'a AgentUnixIdentity,
        mount_table: &'a MountTable,
        agent_subject: &'a str,
        policy: &'a PolicyV0,
    ) -> Self {
        Self {
            identity,
            mount_table,
            agent_subject,
            policy,
        }
    }
}

/// Inputs that define an agent's effective authority for durable session access.
#[derive(Clone, Copy, Debug)]
pub struct SessionAccessAuthority<'a> {
    identity: &'a AgentUnixIdentity,
    mount_table: &'a MountTable,
    agent_subject: &'a str,
    policy: &'a PolicyV0,
}

impl<'a> SessionAccessAuthority<'a> {
    /// Creates an authority context for one private or shared session access.
    #[must_use]
    pub const fn new(
        identity: &'a AgentUnixIdentity,
        mount_table: &'a MountTable,
        agent_subject: &'a str,
        policy: &'a PolicyV0,
    ) -> Self {
        Self {
            identity,
            mount_table,
            agent_subject,
            policy,
        }
    }
}

/// v1 child lifecycle value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildLifecycle {
    /// Parent-owned child. v1 stable supports only this lifecycle.
    Owned,
}

impl ChildLifecycle {
    /// Parses `agent/<child>.d/life`.
    pub fn parse(value: &str) -> Result<Self, ChildAgentDenial> {
        match value.trim() {
            "owned" => Ok(Self::Owned),
            _ => Err(ChildAgentDenial::UnsupportedLifecycle),
        }
    }
}

/// Child control values that carry attenuable authority.
#[derive(Clone, Copy, Debug)]
pub struct ChildAgentControls<'a> {
    identity: &'a AgentUnixIdentity,
    subject: &'a str,
    policy: &'a PolicyV0,
    mounts: &'a MountTable,
}

impl<'a> ChildAgentControls<'a> {
    /// Creates child control values from `uid/gid/groups`, `label`, `policy`,
    /// and `mount`.
    #[must_use]
    pub const fn new(
        identity: &'a AgentUnixIdentity,
        subject: &'a str,
        policy: &'a PolicyV0,
        mounts: &'a MountTable,
    ) -> Self {
        Self {
            identity,
            subject,
            policy,
            mounts,
        }
    }
}

/// Child-agent creation or validation request.
#[derive(Clone, Copy, Debug)]
pub struct ChildAgentRequest<'a> {
    child_name: &'a str,
    parent_ref: &'a str,
    lifecycle: ChildLifecycle,
    controls: ChildAgentControls<'a>,
}

impl<'a> ChildAgentRequest<'a> {
    /// Creates a child request from ordinary child agent control values.
    #[must_use]
    pub const fn new(
        child_name: &'a str,
        parent_ref: &'a str,
        lifecycle: ChildLifecycle,
        controls: ChildAgentControls<'a>,
    ) -> Self {
        Self {
            child_name,
            parent_ref,
            lifecycle,
            controls,
        }
    }
}

/// Parent effective authority used to attenuate a child agent.
#[derive(Clone, Copy, Debug)]
pub struct ChildAgentAuthority<'a> {
    parent_agent: &'a str,
    identity: &'a AgentUnixIdentity,
    subject: &'a str,
    effective_policy: &'a PolicyV0,
    visible_mounts: &'a MountTable,
}

impl<'a> ChildAgentAuthority<'a> {
    /// Creates a parent authority context for child attenuation.
    #[must_use]
    pub const fn new(
        parent_agent: &'a str,
        identity: &'a AgentUnixIdentity,
        subject: &'a str,
        effective_policy: &'a PolicyV0,
        visible_mounts: &'a MountTable,
    ) -> Self {
        Self {
            parent_agent,
            identity,
            subject,
            effective_policy,
            visible_mounts,
        }
    }
}

/// Child-agent attenuation refusal reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildAgentDenial {
    /// Child agent name is not a valid object name.
    InvalidChildName,
    /// Parent agent name is not a valid object name.
    InvalidParentName,
    /// Child subject or parent subject is not a valid policy subject token.
    InvalidSubject,
    /// `agent/<child>.d/parent` does not point at the creating parent.
    ParentMismatch,
    /// Parent reference syntax is invalid.
    InvalidParentRef,
    /// Child lifecycle is not v1 `owned`.
    UnsupportedLifecycle,
    /// Child uid or gid differs from the parent without supervisor authority.
    IdentityExpansion,
    /// Child supplementary groups are not a subset of the parent's groups.
    GroupExpansion,
    /// Child policy grants authority the parent subject does not have.
    PolicyExpansion,
    /// Child mount table exposes paths or permissions outside the parent view.
    MountExpansion,
}

/// Runtime-owned child cancellation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnedChildCancellationError {
    /// Parent agent name is not a valid v1 object name.
    InvalidParentName,
    /// Child agent name is not a valid v1 object name.
    InvalidChildName,
    /// The child session directory is missing durable history files.
    MissingChildHistory,
    /// The parent session event log is missing.
    MissingParentEvents,
    /// Session state or event files could not be updated.
    CannotRecord,
}

/// Stable parent-side child coordination status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildContextStatus {
    /// Parent has prepared handoff context but the child has not completed.
    Pending,
    /// Child runtime is active.
    Active,
    /// Child returned a result successfully.
    Done,
    /// Child failed and returned an inspectable error result.
    Error,
    /// Child runtime was cancelled; history remains durable.
    Cancelled,
}

impl ChildContextStatus {
    /// Parses a stable child context status value.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "active" => Some(Self::Active),
            "done" => Some(Self::Done),
            "error" => Some(Self::Error),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }

    /// Returns the stable status word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Done => "done",
            Self::Error => "error",
            Self::Cancelled => "cancelled",
        }
    }
}

/// Parent-side child handoff/result recording failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChildContextRecordError {
    /// Child coordination directory name is not a valid object name.
    InvalidChildName,
    /// Agent name stored in `context/child/<child>/agent` is invalid.
    InvalidAgentName,
    /// Child session name is invalid.
    InvalidSessionName,
    /// Child result status is not a terminal result status.
    InvalidStatus,
    /// Handoff or result text contains a NUL byte.
    InvalidText,
    /// `refs.jsonl` is not valid stable context refs JSONL.
    InvalidRefs,
    /// Parent session or its context directory is missing required files.
    MissingParentSession,
    /// Child coordination files could not be written.
    CannotRecord,
}

impl ChildContextRecordError {
    /// Returns a stable errno name for this child context recording failure.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidChildName
            | Self::InvalidAgentName
            | Self::InvalidSessionName
            | Self::InvalidStatus
            | Self::InvalidText
            | Self::InvalidRefs => "EINVAL",
            Self::MissingParentSession => "ENOENT",
            Self::CannotRecord => "EIO",
        }
    }
}

impl OwnedChildCancellationError {
    /// Returns a stable errno name for this cancellation failure.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidParentName | Self::InvalidChildName => "EINVAL",
            Self::MissingChildHistory | Self::MissingParentEvents => "ENOENT",
            Self::CannotRecord => "EIO",
        }
    }
}

/// Canonical events emitted when parent death cancels an owned child runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OwnedChildCancellationEvents {
    parent_event: String,
    child_event: String,
}

impl OwnedChildCancellationEvents {
    /// Returns the parent session event.
    #[must_use]
    pub fn parent_event(&self) -> &str {
        &self.parent_event
    }

    /// Returns the child session event.
    #[must_use]
    pub fn child_event(&self) -> &str {
        &self.child_event
    }

    /// Returns both events as canonical JSONL.
    #[must_use]
    pub fn jsonl(&self) -> String {
        format!("{}\n{}\n", self.parent_event, self.child_event)
    }
}

impl ObjectLayoutReport {
    /// Creates a report with collected layout issues.
    #[must_use]
    pub const fn new(issues: Vec<ObjectLayoutIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when the object satisfies the v1 layout.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected layout issues.
    #[must_use]
    pub fn issues(&self) -> &[ObjectLayoutIssue] {
        &self.issues
    }
}

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

impl FuseV1Attr {
    /// Creates a projected file attribute record.
    #[must_use]
    pub const fn new(abi_path: String, file_type: FuseV1FileType, size: u64, mode: u32) -> Self {
        Self::with_owner(abi_path, file_type, size, mode, 0, 0)
    }

    /// Creates a projected file attribute record with source ownership.
    #[must_use]
    pub const fn with_owner(
        abi_path: String,
        file_type: FuseV1FileType,
        size: u64,
        mode: u32,
        uid: u32,
        gid: u32,
    ) -> Self {
        Self {
            abi_path,
            file_type,
            size,
            mode,
            uid,
            gid,
        }
    }

    /// Returns the ABI path relative to `/ctx`.
    #[must_use]
    pub fn abi_path(&self) -> &str {
        &self.abi_path
    }

    /// Returns the projected file kind.
    #[must_use]
    pub const fn file_type(&self) -> FuseV1FileType {
        self.file_type
    }

    /// Returns the projected byte size.
    #[must_use]
    pub const fn size(&self) -> u64 {
        self.size
    }

    /// Returns Unix mode bits from the backing object.
    #[must_use]
    pub const fn mode(&self) -> u32 {
        self.mode
    }

    /// Returns the Unix owner uid from the backing object.
    #[must_use]
    pub const fn uid(&self) -> u32 {
        self.uid
    }

    /// Returns the Unix owner gid from the backing object.
    #[must_use]
    pub const fn gid(&self) -> u32 {
        self.gid
    }
}

impl FuseV1DirEntry {
    /// Creates a projected directory entry.
    #[must_use]
    pub const fn new(name: String, file_type: FuseV1FileType) -> Self {
        Self { name, file_type }
    }

    /// Returns the entry name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the entry file kind.
    #[must_use]
    pub const fn file_type(&self) -> FuseV1FileType {
        self.file_type
    }
}

impl FuseV1Node {
    /// Creates a projected node record.
    #[must_use]
    pub const fn new(inode: u64, abi_path: String, attr: FuseV1Attr) -> Self {
        Self {
            inode,
            abi_path,
            attr,
        }
    }

    /// Returns the stable inode id.
    #[must_use]
    pub const fn inode(&self) -> u64 {
        self.inode
    }

    /// Returns the ABI path relative to `/ctx`.
    #[must_use]
    pub fn abi_path(&self) -> &str {
        &self.abi_path
    }

    /// Returns projected attributes for this node.
    #[must_use]
    pub const fn attr(&self) -> &FuseV1Attr {
        &self.attr
    }
}

impl FuseV1Error {
    /// Returns the stable errno name for this projection error.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidPath
            | Self::NotControlFile
            | Self::InvalidOffset
            | Self::InvalidContent => "EINVAL",
            Self::NotFound => "ENOENT",
            Self::NotDirectory => "ENOTDIR",
            Self::NotFile => "EISDIR",
            Self::TooLarge => "EMSGSIZE",
            Self::Io => "EIO",
        }
    }
}

impl FuseV1Projection {
    /// Creates a local projection over a `/ctx`-shaped root.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            provider_config_dir: PathBuf::from(SYSTEM_PROVIDER_CONFIG_DIR),
        }
    }

    /// Overrides the provider config directory used for projected models.
    #[must_use]
    pub fn with_provider_config_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.provider_config_dir = path.into();
        self
    }

    /// Returns the backing root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Projects `getattr`.
    pub fn getattr(&self, abi_path: &str) -> Result<FuseV1Attr, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(attr) = self.virtual_object_attr(&normalized)? {
            return Ok(attr);
        }
        let path = self.resolve(&normalized)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        Ok(FuseV1Attr::with_owner(
            normalized,
            fuse_file_type(metadata.file_type()),
            metadata.len(),
            metadata.permissions().mode(),
            metadata.uid(),
            metadata.gid(),
        ))
    }

    /// Returns the projected root node.
    pub fn root_node(&self) -> Result<FuseV1Node, FuseV1Error> {
        self.node_for_path("")
    }

    /// Returns the projected node for an ABI path.
    pub fn node_for_path(&self, abi_path: &str) -> Result<FuseV1Node, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        let attr = self.getattr(&normalized)?;
        Ok(FuseV1Node::new(
            fuse_v1_inode_for_path(&normalized),
            normalized,
            attr,
        ))
    }

    /// Projects parent/name lookup.
    pub fn lookup(&self, parent: &FuseV1Node, name: &str) -> Result<FuseV1Node, FuseV1Error> {
        let child = fuse_join_child_path(parent.abi_path(), name)?;
        self.node_for_path(&child)
    }

    /// Projects `getattr` for a known node.
    pub fn getattr_node(&self, node: &FuseV1Node) -> Result<FuseV1Attr, FuseV1Error> {
        self.getattr(node.abi_path())
    }

    /// Projects `readdir`.
    pub fn readdir(&self, abi_path: &str) -> Result<Vec<FuseV1DirEntry>, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(entries) = self.virtual_model_readdir(&normalized)? {
            return Ok(entries);
        }
        let path = self.resolve(&normalized)?;
        if !path.is_dir() {
            return Err(FuseV1Error::NotDirectory);
        }
        let entries = fs::read_dir(&path).map_err(|_error| FuseV1Error::Io)?;
        let mut output = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_error| FuseV1Error::Io)?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|_error| FuseV1Error::InvalidPath)?;
            let metadata =
                fs::symlink_metadata(entry.path()).map_err(|error| fuse_metadata_error(&error))?;
            output.push(FuseV1DirEntry::new(
                name,
                fuse_file_type(metadata.file_type()),
            ));
        }
        output.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(output)
    }

    /// Projects `readdir` for a known node.
    pub fn readdir_node(&self, node: &FuseV1Node) -> Result<Vec<FuseV1DirEntry>, FuseV1Error> {
        self.readdir(node.abi_path())
    }

    /// Projects a small text `read`.
    pub fn read_to_string(&self, abi_path: &str) -> Result<String, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(content) = self.virtual_object_content(&normalized)? {
            return Ok(content);
        }
        let path = self.resolve(&normalized)?;
        if path.is_dir() {
            return Err(FuseV1Error::NotFile);
        }
        fs::read_to_string(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                FuseV1Error::NotFound
            } else {
                FuseV1Error::Io
            }
        })
    }

    /// Projects an offset `read`.
    pub fn read_at(
        &self,
        abi_path: &str,
        offset: u64,
        size: usize,
    ) -> Result<Vec<u8>, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(content) = self.virtual_object_content(&normalized)? {
            return read_bytes_at(content.as_bytes(), offset, size);
        }
        let path = self.resolve(&normalized)?;
        if path.is_dir() {
            return Err(FuseV1Error::NotFile);
        }
        let mut file = fs::File::open(path).map_err(|error| fuse_metadata_error(&error))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|_error| FuseV1Error::Io)?;
        let mut buffer = vec![0; size];
        let read = file.read(&mut buffer).map_err(|_error| FuseV1Error::Io)?;
        buffer.truncate(read);
        Ok(buffer)
    }

    /// Projects a symlink target.
    pub fn readlink(&self, abi_path: &str) -> Result<PathBuf, FuseV1Error> {
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if let Some(alias) = model_alias_name(&normalized) {
            return self.default_model_alias_target(alias);
        }
        let path = self.resolve(&normalized)?;
        fs::read_link(path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                FuseV1Error::NotFound
            } else {
                FuseV1Error::InvalidPath
            }
        })
    }

    fn virtual_object_attr(&self, abi_path: &str) -> Result<Option<FuseV1Attr>, FuseV1Error> {
        if let Some((file_type, size, mode)) = self.virtual_model_entry(abi_path)? {
            return Ok(Some(FuseV1Attr::with_owner(
                abi_path.to_owned(),
                file_type,
                size,
                mode,
                0,
                0,
            )));
        }
        let Some(content) = self.virtual_object_content(abi_path)? else {
            return Ok(None);
        };
        Ok(Some(FuseV1Attr::with_owner(
            abi_path.to_owned(),
            FuseV1FileType::Regular,
            u64::try_from(content.len()).map_err(|_error| FuseV1Error::Io)?,
            0o555,
            0,
            0,
        )))
    }

    fn virtual_model_readdir(
        &self,
        abi_path: &str,
    ) -> Result<Option<Vec<FuseV1DirEntry>>, FuseV1Error> {
        let mut entries = match abi_path {
            "model" => {
                let mut provider_names = HashSet::from([DEBUG_ECHO_PROVIDER.to_owned()]);
                let model_root = self.root.join("model");
                if model_root.is_dir() {
                    for name in read_model_provider_dirs(&model_root)? {
                        provider_names.insert(name);
                    }
                }
                for provider in projected_provider_models(&self.provider_config_dir)?
                    .into_iter()
                    .map(|model| model.provider)
                {
                    provider_names.insert(provider);
                }
                let mut entries = provider_names
                    .into_iter()
                    .map(|provider| FuseV1DirEntry::new(provider, FuseV1FileType::Directory))
                    .collect::<Vec<_>>();
                entries.push(FuseV1DirEntry::new(
                    DEFAULT_MODEL_ALIAS.to_owned(),
                    FuseV1FileType::Symlink,
                ));
                entries.push(FuseV1DirEntry::new(
                    HELPER_MODEL_ALIAS.to_owned(),
                    FuseV1FileType::Symlink,
                ));
                entries
            }
            "model/debug" => vec![
                FuseV1DirEntry::new(DEBUG_ECHO_NAME.to_owned(), FuseV1FileType::Regular),
                FuseV1DirEntry::new(format!("{DEBUG_ECHO_NAME}.d"), FuseV1FileType::Directory),
            ],
            "model/debug/echo.d" => MODEL_CONTROL_FILES
                .iter()
                .map(|file| FuseV1DirEntry::new((*file).to_owned(), FuseV1FileType::Regular))
                .collect(),
            _ => {
                if let Some(model) =
                    projected_provider_model_control_dir(&self.provider_config_dir, abi_path)?
                {
                    let _ = model;
                    MODEL_CONTROL_FILES
                        .iter()
                        .map(|file| {
                            FuseV1DirEntry::new((*file).to_owned(), FuseV1FileType::Regular)
                        })
                        .collect()
                } else if let Some(provider) = abi_path.strip_prefix("model/") {
                    if provider.contains('/') || provider == DEBUG_ECHO_PROVIDER {
                        return Ok(None);
                    }
                    let models = projected_provider_models_for_provider(
                        &self.provider_config_dir,
                        provider,
                    )?;
                    if models.is_empty() {
                        return Ok(None);
                    }
                    let mut entries = Vec::new();
                    for model in models {
                        entries.push(FuseV1DirEntry::new(
                            model.model.clone(),
                            FuseV1FileType::Regular,
                        ));
                        entries.push(FuseV1DirEntry::new(
                            format!("{}.d", model.model),
                            FuseV1FileType::Directory,
                        ));
                    }
                    entries
                } else {
                    return Ok(None);
                }
            }
        };
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(Some(entries))
    }

    fn virtual_object_content(&self, abi_path: &str) -> Result<Option<String>, FuseV1Error> {
        if let Some(content) = self.virtual_model_content(abi_path)? {
            return Ok(Some(content));
        }
        let Some(object) = self.virtual_exec_object(abi_path) else {
            return Ok(None);
        };
        object_exec_metadata(object.class, &object.name, &object.control_dir).map(Some)
    }

    fn virtual_model_content(&self, abi_path: &str) -> Result<Option<String>, FuseV1Error> {
        if abi_path == "model/debug/echo" {
            return Ok(Some(debug_echo_model_metadata()));
        }
        if let Some(file) = abi_path.strip_prefix("model/debug/echo.d/") {
            return Ok(debug_echo_control_content(file).map(str::to_owned));
        }
        let Some(model) = projected_provider_model_for_exec(&self.provider_config_dir, abi_path)?
        else {
            let Some((model, file)) =
                projected_provider_model_control_file(&self.provider_config_dir, abi_path)?
            else {
                return Ok(None);
            };
            return Ok(provider_model_control_content(&model, file));
        };
        Ok(Some(provider_model_metadata(&model)))
    }

    fn virtual_exec_object(&self, abi_path: &str) -> Option<VirtualExecObject> {
        let (class, name) = parse_abi_path(abi_path).executable_object()?;
        let name = name.into_owned();
        let control_dir = self.root.join(class.as_str()).join(format!("{name}.d"));
        if !control_dir.is_dir() {
            return None;
        }
        Some(VirtualExecObject {
            class,
            name,
            control_dir,
        })
    }

    /// Projects a same-directory atomic write for v1 control files.
    pub fn write_control_file(&self, abi_path: &str, content: &str) -> Result<(), FuseV1Error> {
        self.write_control_file_at(abi_path, 0, content.as_bytes())
    }

    /// Projects an offset write for v1 control files.
    ///
    /// v1 only accepts whole-file, same-directory atomic replacement. A FUSE
    /// adapter should collect one small control-file payload and submit it at
    /// offset zero.
    pub fn write_control_file_at(
        &self,
        abi_path: &str,
        offset: u64,
        content: &[u8],
    ) -> Result<(), FuseV1Error> {
        if offset != 0 {
            return Err(FuseV1Error::InvalidOffset);
        }
        if content.len() > MAX_FUSE_V1_SMALL_WRITE_BYTES {
            return Err(FuseV1Error::TooLarge);
        }
        let normalized = normalize_fuse_abi_path(abi_path)?;
        if !is_fuse_v1_writable_control_path(&normalized) {
            return Err(FuseV1Error::NotControlFile);
        }
        let path = self.resolve(&normalized)?;
        let content = std::str::from_utf8(content).map_err(|_error| FuseV1Error::InvalidContent)?;
        atomic_replace_text(&path, content).map_err(|_error| FuseV1Error::Io)
    }

    fn resolve(&self, abi_path: &str) -> Result<PathBuf, FuseV1Error> {
        resolve_fuse_abi_path(&self.root, abi_path)
    }

    fn virtual_model_entry(
        &self,
        abi_path: &str,
    ) -> Result<Option<(FuseV1FileType, u64, u32)>, FuseV1Error> {
        match abi_path {
            path if model_alias_name(path).is_some() => Ok(Some((
                FuseV1FileType::Symlink,
                u64::try_from(
                    self.default_model_alias_target(model_alias_name(path).unwrap_or_default())?
                        .as_os_str()
                        .len(),
                )
                .map_err(|_error| FuseV1Error::Io)?,
                0o777,
            ))),
            "model/debug" | "model/debug/echo.d" => Ok(Some((FuseV1FileType::Directory, 0, 0o755))),
            "model/debug/echo" => Ok(Some((
                FuseV1FileType::Regular,
                u64::try_from(debug_echo_model_metadata().len())
                    .map_err(|_error| FuseV1Error::Io)?,
                0o555,
            ))),
            path => {
                if let Some(file) = path.strip_prefix("model/debug/echo.d/") {
                    let Some(content) = debug_echo_control_content(file) else {
                        return Ok(None);
                    };
                    return Ok(Some((
                        FuseV1FileType::Regular,
                        u64::try_from(content.len()).map_err(|_error| FuseV1Error::Io)?,
                        0o644,
                    )));
                }
                if projected_provider_models_for_provider_path(&self.provider_config_dir, path)?
                    .is_some()
                {
                    return Ok(Some((FuseV1FileType::Directory, 0, 0o755)));
                }
                if let Some(model) =
                    projected_provider_model_for_exec(&self.provider_config_dir, path)?
                {
                    let content = provider_model_metadata(&model);
                    return Ok(Some((
                        FuseV1FileType::Regular,
                        u64::try_from(content.len()).map_err(|_error| FuseV1Error::Io)?,
                        0o555,
                    )));
                }
                if projected_provider_model_control_dir(&self.provider_config_dir, path)?.is_some()
                {
                    return Ok(Some((FuseV1FileType::Directory, 0, 0o755)));
                }
                let Some((model, file)) =
                    projected_provider_model_control_file(&self.provider_config_dir, path)?
                else {
                    return Ok(None);
                };
                let Some(content) = provider_model_control_content(&model, file) else {
                    return Ok(None);
                };
                Ok(Some((
                    FuseV1FileType::Regular,
                    u64::try_from(content.len()).map_err(|_error| FuseV1Error::Io)?,
                    0o644,
                )))
            }
        }
    }

    fn default_model_alias_target(&self, alias: &str) -> Result<PathBuf, FuseV1Error> {
        let path = self.resolve(&format!("model/{alias}"))?;
        if let Ok(target) = fs::read_link(path)
            && is_valid_ctx_model_symlink(&target)
        {
            return Ok(target);
        }
        Ok(PathBuf::from(DEFAULT_MODEL_ALIAS_TARGET))
    }
}

fn model_alias_name(abi_path: &str) -> Option<&str> {
    let alias = abi_path.strip_prefix("model/")?;
    matches!(alias, DEFAULT_MODEL_ALIAS | HELPER_MODEL_ALIAS).then_some(alias)
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

fn debug_echo_model_metadata() -> String {
    [
        format!("#!{CORTEXFS_OBJECT_RUNNER}"),
        "# cortexfs.object=model".to_owned(),
        "# cortexfs.id=debug/echo".to_owned(),
        "# cortexfs.name=debug/echo".to_owned(),
        "# cortexfs.description=Built-in debug echo model".to_owned(),
        "# cortexfs.type=debug".to_owned(),
        "# cortexfs.created_at=".to_owned(),
        "# cortexfs.owned_by=cortexfs".to_owned(),
        "# cortexfs.context_length=0".to_owned(),
        "# cortexfs.driver=debug".to_owned(),
        "# cortexfs.driver.default=debug".to_owned(),
        "# cortexfs.driver.exec=debug".to_owned(),
        "# cortexfs.driver.socket=".to_owned(),
        "# cortexfs.driver.agent=debug".to_owned(),
        "# cortexfs.session=none".to_owned(),
        "# cortexfs.status=idle".to_owned(),
        "# cortexfs.cap=chat,stream".to_owned(),
    ]
    .join("\n")
        + "\n"
}

fn debug_echo_control_content(file: &str) -> Option<&'static str> {
    match file {
        "id" => Some("debug/echo\n"),
        "driver" => Some("default=debug\nexec=debug\nagent=debug\n"),
        "cap" => Some("chat\nstream\n"),
        "default" | "log" => Some("\n"),
        "session" => Some("none\n"),
        "status" => Some("idle\n"),
        _ => None,
    }
}

fn default_provider_enabled() -> bool {
    true
}

fn projected_provider_models(
    config_dir: &Path,
) -> Result<Vec<ProjectedProviderModel>, FuseV1Error> {
    let entries = match fs::read_dir(config_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_error) => return Err(FuseV1Error::Io),
    };
    let mut projected = Vec::new();
    let mut seen = HashSet::new();
    for entry in entries {
        let entry = entry.map_err(|_error| FuseV1Error::Io)?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        if metadata.file_type().is_dir() {
            continue;
        }
        let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
            continue;
        };
        if extension != "json" {
            continue;
        }
        let content = fs::read_to_string(&path).map_err(|_error| FuseV1Error::Io)?;
        let Ok(config) = serde_json::from_str::<ProviderConfig>(&content) else {
            continue;
        };
        if !config.enabled {
            continue;
        }
        let Some(provider) = provider_name_from_base_url(&config.base_url) else {
            continue;
        };
        let driver = provider_driver_route_table(&config.formats);
        let cap = provider_capability_text(&config.formats);
        for model in provider_config_models(&config) {
            let key = format!("{provider}/{model}");
            if seen.insert(key) {
                projected.push(ProjectedProviderModel {
                    provider: provider.clone(),
                    model,
                    base_url: normalize_provider_base_url(&config.base_url),
                    driver: driver.clone(),
                    cap: cap.clone(),
                });
            }
        }
    }
    projected.sort_by(|left, right| {
        left.provider
            .cmp(&right.provider)
            .then_with(|| left.model.cmp(&right.model))
    });
    Ok(projected)
}

fn projected_provider_models_for_provider(
    config_dir: &Path,
    provider: &str,
) -> Result<Vec<ProjectedProviderModel>, FuseV1Error> {
    Ok(projected_provider_models(config_dir)?
        .into_iter()
        .filter(|model| model.provider == provider)
        .collect())
}

fn projected_provider_models_for_provider_path(
    config_dir: &Path,
    abi_path: &str,
) -> Result<Option<Vec<ProjectedProviderModel>>, FuseV1Error> {
    let Some(provider) = abi_path.strip_prefix("model/") else {
        return Ok(None);
    };
    if provider.contains('/') || provider == DEBUG_ECHO_PROVIDER {
        return Ok(None);
    }
    let models = projected_provider_models_for_provider(config_dir, provider)?;
    if models.is_empty() {
        Ok(None)
    } else {
        Ok(Some(models))
    }
}

fn projected_provider_model_for_exec(
    config_dir: &Path,
    abi_path: &str,
) -> Result<Option<ProjectedProviderModel>, FuseV1Error> {
    let Some(model_name) = model_exec_name(abi_path) else {
        return Ok(None);
    };
    Ok(projected_provider_models(config_dir)?
        .into_iter()
        .find(|model| format!("{}/{}", model.provider, model.model) == model_name))
}

fn projected_provider_model_control_dir(
    config_dir: &Path,
    abi_path: &str,
) -> Result<Option<ProjectedProviderModel>, FuseV1Error> {
    let Some(model_name) = abi_path
        .strip_prefix("model/")
        .and_then(|path| path.strip_suffix(".d"))
    else {
        return Ok(None);
    };
    if !is_model_name(model_name) {
        return Ok(None);
    }
    Ok(projected_provider_models(config_dir)?
        .into_iter()
        .find(|model| format!("{}/{}", model.provider, model.model) == model_name))
}

fn projected_provider_model_control_file<'a>(
    config_dir: &Path,
    abi_path: &'a str,
) -> Result<Option<(ProjectedProviderModel, &'a str)>, FuseV1Error> {
    let Some((dir, file)) = abi_path.rsplit_once('/') else {
        return Ok(None);
    };
    let Some(model) = projected_provider_model_control_dir(config_dir, dir)? else {
        return Ok(None);
    };
    Ok(Some((model, file)))
}

fn provider_config_models(config: &ProviderConfig) -> Vec<String> {
    let mut models = Vec::new();
    let mut seen = HashSet::new();
    if let Some(model) = config.default_model.as_deref() {
        append_provider_model_name(model, &mut models, &mut seen);
    }
    for model in &config.models {
        append_provider_model_name(model, &mut models, &mut seen);
    }
    models
}

fn append_provider_model_name(model: &str, models: &mut Vec<String>, seen: &mut HashSet<String>) {
    let model = model.trim();
    if !is_object_name(model) {
        return;
    }
    if seen.insert(model.to_owned()) {
        models.push(model.to_owned());
    }
}

fn provider_name_from_base_url(base_url: &str) -> Option<String> {
    let mut rest = base_url.trim();
    if let Some(value) = rest.strip_prefix("https://") {
        rest = value;
    } else if let Some(value) = rest.strip_prefix("http://") {
        rest = value;
    }
    let authority = rest
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('@')
        .next()
        .unwrap_or_default();
    let host = authority
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    is_object_name(&host).then_some(host)
}

fn normalize_provider_base_url(base_url: &str) -> String {
    base_url.trim().to_owned()
}

fn provider_driver_route_table(formats: &[String]) -> String {
    let drivers = provider_drivers(formats);
    let default = drivers
        .iter()
        .find(|driver| driver.as_str() == "openai-chat")
        .or_else(|| drivers.first())
        .map_or("openai-chat", String::as_str);
    let agent = if drivers.iter().any(|driver| driver == "openai-responses")
        && drivers.iter().any(|driver| driver == "openai-chat")
    {
        "openai-responses,openai-chat".to_owned()
    } else {
        default.to_owned()
    };
    format!("default={default}\nexec={default}\nagent={agent}\n")
}

fn provider_drivers(formats: &[String]) -> Vec<String> {
    let mut drivers = Vec::new();
    let mut seen = HashSet::new();
    for format in formats {
        let driver = match format.trim() {
            "openai.responses" => "openai-responses",
            "openai.chat" | "openai-compatible" => "openai-chat",
            _ => continue,
        };
        if seen.insert(driver) {
            drivers.push(driver.to_owned());
        }
    }
    if drivers.is_empty() {
        drivers.push("openai-chat".to_owned());
    }
    drivers
}

fn provider_capability_text(formats: &[String]) -> String {
    let mut capabilities = vec!["chat", "stream"];
    if formats
        .iter()
        .any(|format| format.trim() == "openai.responses")
    {
        capabilities.push("tool_call_syntax");
    }
    capabilities.join("\n") + "\n"
}

fn provider_model_metadata(model: &ProjectedProviderModel) -> String {
    let name = format!("{}/{}", model.provider, model.model);
    let routes = parse_model_driver_routes(&model.driver).unwrap_or_default();
    let driver = routes
        .primary_driver_for(ModelDriverUseCase::Default)
        .unwrap_or("openai-chat");
    format!(
        "#!{CORTEXFS_OBJECT_RUNNER}\n\
         # cortexfs.object=model\n\
         # cortexfs.id={name}\n\
         # cortexfs.name={name}\n\
         # cortexfs.description=Configured provider model\n\
         # cortexfs.type=chat\n\
         # cortexfs.created_at=\n\
         # cortexfs.owned_by={}\n\
         # cortexfs.context_length=0\n\
         # cortexfs.driver={driver}\n\
         # cortexfs.driver.default={}\n\
         # cortexfs.driver.exec={}\n\
         # cortexfs.driver.socket={}\n\
         # cortexfs.driver.agent={}\n\
         # cortexfs.session=none\n\
         # cortexfs.status=configured\n\
         # cortexfs.cap={}\n",
        model.provider,
        routes.route_value(ModelDriverUseCase::Default),
        routes.route_value(ModelDriverUseCase::Exec),
        routes.route_value(ModelDriverUseCase::Socket),
        routes.route_value(ModelDriverUseCase::Agent),
        model.cap.lines().collect::<Vec<_>>().join(",")
    )
}

fn provider_model_control_content(model: &ProjectedProviderModel, file: &str) -> Option<String> {
    match file {
        "id" => Some(format!("{}/{}\n", model.provider, model.model)),
        "driver" => Some(model.driver.clone()),
        "cap" => Some(model.cap.clone()),
        "default" => Some(format!("base_url={}\n", model.base_url)),
        "session" => Some("none\n".to_owned()),
        "status" => Some("configured\n".to_owned()),
        "log" => Some("\n".to_owned()),
        _ => None,
    }
}

fn read_model_provider_dirs(model_root: &Path) -> Result<Vec<String>, FuseV1Error> {
    let entries = fs::read_dir(model_root).map_err(|_error| FuseV1Error::Io)?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_error| FuseV1Error::Io)?;
        let metadata =
            fs::symlink_metadata(entry.path()).map_err(|error| fuse_metadata_error(&error))?;
        if !metadata.is_dir() {
            continue;
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_error| FuseV1Error::InvalidPath)?;
        if is_object_name(&name) {
            names.push(name);
        }
    }
    names.sort();
    Ok(names)
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
        && !is_model_name(model)
    {
        return Err(DurableSessionLayoutError::InvalidModelName);
    }
    if scope == SocketSessionScope::Temp {
        return Err(DurableSessionLayoutError::TempSessionNotDurable);
    }

    let session_dir = session_root.join(session_name);
    let context = session_dir.join("context");
    create_dir(&session_dir)?;
    create_dir(&context)?;
    for dir in CONTEXT_REQUIRED_DIRS {
        create_dir(&context.join(dir))?;
    }
    create_dir(&context.join("swap").join("chunk"))?;
    create_dir(&context.join("dedup").join("blob"))?;
    create_dir(&session_root.join("index").join("by-cwd"))?;

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
    let value = model.map_or_else(
        || {
            serde_json::json!({
                "client": "ctx",
                "scope": scope.as_str()
            })
        },
        |model| {
            serde_json::json!({
            "client": "ctx",
            "model": model,
            "scope": scope.as_str()
            })
        },
    );
    format!("{value}\n")
}

fn create_dir(path: &Path) -> Result<(), DurableSessionLayoutError> {
    fs::create_dir_all(path).map_err(|_error| DurableSessionLayoutError::CannotCreate)
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
    fs::write(path, content).map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    set_text_file_permissions(path)
}

fn write_text_file(path: &Path, content: &str) -> Result<(), DurableSessionLayoutError> {
    if let Some(parent) = path.parent() {
        create_dir(parent)?;
    }
    fs::write(path, content).map_err(|_error| DurableSessionLayoutError::CannotCreate)?;
    set_text_file_permissions(path)
}

fn set_text_file_permissions(path: &Path) -> Result<(), DurableSessionLayoutError> {
    fs::set_permissions(path, fs::Permissions::from_mode(0o644))
        .map_err(|_error| DurableSessionLayoutError::CannotCreate)
}

/// Handles one JSONL socket request frame against a durable session root.
///
/// This is the reusable core for a future Unix socket loop: it parses one
/// request, applies `CortexFS` session-file semantics, and returns canonical
/// response frames. It does not call a model provider and does not execute
/// tools.
pub fn handle_socket_request_frame(
    session_root: &Path,
    default_cwd: &str,
    model: Option<&str>,
    frame: &str,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let request = parse_socket_request_frame(frame).map_err(SocketRuntimeError::Request)?;
    handle_socket_request(session_root, default_cwd, model, &request)
}

/// Handles one parsed socket request against a durable session root.
pub fn handle_socket_request(
    session_root: &Path,
    default_cwd: &str,
    model: Option<&str>,
    request: &SocketRequest,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    match *request {
        SocketRequest::Ping => Ok(SocketRuntimeResponse::new(vec![socket_pong_frame()])),
        SocketRequest::Send { .. } => handle_socket_send(session_root, default_cwd, model, request),
        SocketRequest::Resume {
            ref session,
            ref after,
        } => handle_socket_resume(session_root, session, after.as_deref()),
        SocketRequest::Cancel { ref id } => handle_socket_cancel(session_root, id),
    }
}

/// Builds a canonical socket error response frame from a runtime error.
#[must_use]
pub fn socket_runtime_error_response(error: &SocketRuntimeError) -> SocketRuntimeResponse {
    SocketRuntimeResponse::new(vec![
        serde_json::json!({
            "type": "error",
            "code": error.errno(),
            "message": error.errno()
        })
        .to_string(),
    ])
}

/// Accepts and serves one Unix socket connection.
///
/// This is a bounded runtime helper for `name.sock` implementations. It does
/// not loop, spawn, supervise, or watch files; callers decide process lifetime.
pub fn serve_unix_socket_listener_once(
    listener: &UnixListener,
    peer_policy: Option<SocketPeerPolicy>,
    session_root: &Path,
    default_cwd: &str,
    model: Option<&str>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let (mut stream, _addr) = listener
        .accept()
        .map_err(|_error| SocketRuntimeError::CannotAcceptConnection)?;
    serve_unix_socket_stream_once(&mut stream, peer_policy, session_root, default_cwd, model)
}

/// Accepts one Unix socket connection and dispatches `send` to an agent executable.
///
/// This is the reference socket-activated agent runtime path. It preserves the
/// durable socket request semantics, then runs the ABI executable object for
/// `send` requests and returns its canonical JSONL events to the client.
pub fn serve_agent_executable_socket_listener_once(
    listener: &UnixListener,
    peer_policy: Option<SocketPeerPolicy>,
    runtime: AgentExecutableSocketRuntime<'_>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let (mut stream, _addr) = listener
        .accept()
        .map_err(|_error| SocketRuntimeError::CannotAcceptConnection)?;
    serve_agent_executable_socket_stream_once(&mut stream, peer_policy, runtime)
}

/// Serves one connected stream and dispatches `send` to an agent executable.
pub fn serve_agent_executable_socket_stream_once(
    stream: &mut UnixStream,
    peer_policy: Option<SocketPeerPolicy>,
    runtime: AgentExecutableSocketRuntime<'_>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    if let Some(policy) = peer_policy {
        let peer = peer_credentials(stream).map_err(SocketRuntimeError::PeerCredential)?;
        if !policy.allows(peer) {
            let error = SocketRuntimeError::PeerDenied;
            let response = socket_runtime_error_response(&error);
            write_socket_runtime_response(stream, &response)?;
            return Err(error);
        }
    }

    let frame = match read_socket_request_frame_from_stream(stream) {
        Ok(frame) => frame,
        Err(error) => {
            let response = socket_runtime_error_response(&error);
            write_socket_runtime_response(stream, &response)?;
            return Err(error);
        }
    };
    match handle_agent_executable_socket_request_frame_streaming(stream, runtime, &frame) {
        Ok(response) => Ok(response),
        Err(error) => {
            let response = socket_runtime_error_response(&error);
            write_socket_runtime_response(stream, &response)?;
            Err(error)
        }
    }
}

/// Serves one connected Unix socket stream request.
///
/// This helper enforces optional kernel peer credentials before reading a
/// single JSONL frame, then writes either the request response or a stable
/// error frame. It is intentionally one-shot; process supervision and accept
/// loops remain outside the ABI.
pub fn serve_unix_socket_stream_once(
    stream: &mut UnixStream,
    peer_policy: Option<SocketPeerPolicy>,
    session_root: &Path,
    default_cwd: &str,
    model: Option<&str>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    if let Some(policy) = peer_policy {
        let peer = peer_credentials(stream).map_err(SocketRuntimeError::PeerCredential)?;
        if !policy.allows(peer) {
            let error = SocketRuntimeError::PeerDenied;
            let response = socket_runtime_error_response(&error);
            write_socket_runtime_response(stream, &response)?;
            return Err(error);
        }
    }

    let frame = match read_socket_request_frame_from_stream(stream) {
        Ok(frame) => frame,
        Err(error) => {
            let response = socket_runtime_error_response(&error);
            write_socket_runtime_response(stream, &response)?;
            return Err(error);
        }
    };
    match handle_socket_request_frame(session_root, default_cwd, model, &frame) {
        Ok(response) => {
            write_socket_runtime_response(stream, &response)?;
            Ok(response)
        }
        Err(error) => {
            let response = socket_runtime_error_response(&error);
            write_socket_runtime_response(stream, &response)?;
            Err(error)
        }
    }
}

fn handle_agent_executable_socket_request_frame_streaming(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    frame: &str,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let request = parse_socket_request_frame(frame).map_err(SocketRuntimeError::Request)?;
    let SocketRequest::Send {
        ref id,
        ref session,
        scope,
        ref input,
        ..
    } = request
    else {
        let response = handle_socket_request(
            runtime.session_root,
            runtime.default_cwd,
            runtime.model,
            &request,
        )?;
        write_socket_runtime_response(stream, &response)?;
        return Ok(response);
    };

    let recorder_response = handle_socket_request(
        runtime.session_root,
        runtime.default_cwd,
        runtime.model,
        &request,
    )?;
    write_socket_runtime_response(stream, &recorder_response)?;

    let agent_frames = run_agent_executable_streaming(stream, runtime, id, session, input)?;
    if scope != SocketSessionScope::Temp
        && let Some(text) = assistant_text_from_event_frames(&agent_frames)
    {
        let session_dir = runtime.session_root.join(session);
        record_assistant_response_to_session(&session_dir, id, &text)
            .map_err(SocketRuntimeError::Record)?;
    }

    let mut frames = recorder_response.frames().to_vec();
    frames.extend(agent_frames);
    Ok(SocketRuntimeResponse::new(frames))
}

fn run_agent_executable_streaming(
    stream: &mut UnixStream,
    runtime: AgentExecutableSocketRuntime<'_>,
    run_id: &str,
    session: &str,
    input: &str,
) -> Result<Vec<String>, SocketRuntimeError> {
    let mut child = Command::new(runtime.agent_executable)
        .arg(input)
        .env("CTX_AGENT", runtime.agent_name)
        .env("CTX_ROOT", runtime.ctx_root)
        .env("CTX_SOURCE", runtime.source_root)
        .env("CTX_RUN_ID", run_id)
        .env("CTX_SESSION", session)
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    let stdout = child
        .stdout
        .take()
        .ok_or(SocketRuntimeError::CannotRunAgent)?;
    let mut frames = Vec::new();
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|_error| SocketRuntimeError::CannotReadFrame)?;
        if line.trim().is_empty() {
            continue;
        }
        if !inspect_event_stream_jsonl(&line).is_ok() {
            return Err(SocketRuntimeError::InvalidAgentOutput);
        }
        if event_type(&line).as_deref() != Some("start") {
            write_socket_frame(stream, &line)?;
            frames.push(line);
        }
    }
    let status = child
        .wait()
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    if !status.success() && frames.is_empty() {
        return Err(SocketRuntimeError::CannotRunAgent);
    }
    Ok(frames)
}

fn event_type(line: &str) -> Option<String> {
    serde_json::from_str::<Value>(line)
        .ok()
        .and_then(|value| value.get("type").and_then(Value::as_str).map(str::to_owned))
}

fn assistant_text_from_event_frames(frames: &[String]) -> Option<String> {
    let mut output = String::new();
    for frame in frames {
        let Ok(value) = serde_json::from_str::<Value>(frame) else {
            continue;
        };
        let event_type = value.get("type").and_then(Value::as_str);
        if matches!(event_type, Some("delta" | "reasoning_delta"))
            && let Some(text) = value.get("text").and_then(Value::as_str)
        {
            output.push_str(text);
            continue;
        }
        if matches!(event_type, Some("message" | "reasoning_message"))
            && value.get("role").and_then(Value::as_str) == Some("assistant")
            && let Some(text) = message_event_text(&value)
        {
            if !output.is_empty() {
                output.push('\n');
            }
            output.push_str(&text);
        }
    }
    (!output.is_empty()).then_some(output)
}

fn message_event_text(value: &Value) -> Option<String> {
    let parts = value.get("content")?.as_array()?;
    let mut text = String::new();
    for part in parts {
        if part.get("type").and_then(Value::as_str) == Some("text")
            && let Some(value) = part.get("text").and_then(Value::as_str)
        {
            text.push_str(value);
        }
    }
    (!text.is_empty()).then_some(text)
}

fn read_socket_request_frame_from_stream(
    stream: &mut UnixStream,
) -> Result<String, SocketRuntimeError> {
    let mut buffer = Vec::new();
    let mut byte = [0_u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => {
                buffer.push(byte[0]);
                if buffer.len() > MAX_SOCKET_FRAME_BYTES {
                    return Err(SocketRuntimeError::Request(
                        SocketRequestError::FrameTooLarge {
                            bytes: buffer.len(),
                        },
                    ));
                }
                if byte[0] == b'\n' {
                    break;
                }
            }
            Err(_error) => return Err(SocketRuntimeError::CannotReadFrame),
        }
    }
    String::from_utf8(buffer)
        .map_err(|_error| SocketRuntimeError::Request(SocketRequestError::InvalidJson))
}

fn write_socket_runtime_response(
    stream: &mut UnixStream,
    response: &SocketRuntimeResponse,
) -> Result<(), SocketRuntimeError> {
    stream
        .write_all(response.jsonl().as_bytes())
        .and_then(|()| stream.flush())
        .map_err(|_error| SocketRuntimeError::CannotWriteResponse)
}

fn write_socket_frame(stream: &mut UnixStream, frame: &str) -> Result<(), SocketRuntimeError> {
    stream
        .write_all(frame.as_bytes())
        .and_then(|()| stream.write_all(b"\n"))
        .and_then(|()| stream.flush())
        .map_err(|_error| SocketRuntimeError::CannotWriteResponse)
}

fn handle_socket_send(
    session_root: &Path,
    default_cwd: &str,
    model: Option<&str>,
    request: &SocketRequest,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let &SocketRequest::Send {
        ref id,
        ref session,
        scope,
        ref cwd,
        ref input,
    } = request
    else {
        return Err(SocketRuntimeError::Record(
            SocketSessionRecordError::UnsupportedRequest,
        ));
    };
    let effective_cwd = cwd.as_deref().unwrap_or(default_cwd);
    if scope == SocketSessionScope::Temp {
        if !is_stable_chroot_absolute_path(effective_cwd) {
            return Err(SocketRuntimeError::SessionLayout(
                DurableSessionLayoutError::InvalidCwd,
            ));
        }
        return Ok(SocketRuntimeResponse::new(vec![socket_start_frame(
            id, model,
        )]));
    }

    ensure_durable_session_layout(session_root, session, effective_cwd, model, scope)
        .map_err(SocketRuntimeError::SessionLayout)?;
    let durable_request = SocketRequest::Send {
        id: id.to_owned(),
        session: session.to_owned(),
        scope,
        cwd: Some(effective_cwd.to_owned()),
        input: input.to_owned(),
    };
    let record = record_indexed_socket_send_to_session(session_root, &durable_request)
        .map_err(SocketRuntimeError::IndexedRecord)?;
    Ok(SocketRuntimeResponse::new(record.events().to_vec()))
}

fn handle_socket_resume(
    session_root: &Path,
    session: &str,
    after: Option<&str>,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    if !is_object_name(session) {
        return Err(SocketRuntimeError::InvalidSessionName);
    }
    let events = fs::read_to_string(session_root.join(session).join("events.jsonl"))
        .map_err(|_error| SocketRuntimeError::CannotReadEvents)?;
    Ok(SocketRuntimeResponse::new(resume_event_frames(
        &events, after,
    )))
}

fn handle_socket_cancel(
    session_root: &Path,
    run_id: &str,
) -> Result<SocketRuntimeResponse, SocketRuntimeError> {
    let session = current_or_default_session_name(session_root)?;
    let session_dir = session_root.join(session);
    let request = SocketRequest::Cancel {
        id: run_id.to_owned(),
    };
    let record = record_socket_request_to_session(&session_dir, &request)
        .map_err(SocketRuntimeError::Record)?;
    Ok(SocketRuntimeResponse::new(record.events().to_vec()))
}

fn resume_event_frames(events: &str, after: Option<&str>) -> Vec<String> {
    let mut include = after.is_none();
    let mut frames = Vec::new();
    for line in events.lines().filter(|line| !line.trim().is_empty()) {
        if include {
            frames.push(line.to_owned());
            continue;
        }
        if after.is_some_and(|cursor| event_id_matches(line, cursor)) {
            include = true;
        }
    }
    frames
}

fn event_id_matches(line: &str, cursor: &str) -> bool {
    serde_json::from_str::<Value>(line).is_ok_and(|value| {
        value
            .as_object()
            .and_then(|object| object.get("id"))
            .and_then(Value::as_str)
            == Some(cursor)
    })
}

fn current_or_default_session_name(session_root: &Path) -> Result<String, SocketRuntimeError> {
    let current_path = session_root.join("index").join("current");
    match fs::read_to_string(current_path) {
        Ok(value) => {
            let session = value.trim();
            if is_object_name(session) {
                Ok(session.to_owned())
            } else {
                Err(SocketRuntimeError::InvalidSessionName)
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok("default".to_owned()),
        Err(_error) => Err(SocketRuntimeError::CannotReadEvents),
    }
}

fn socket_start_frame(run_id: &str, model: Option<&str>) -> String {
    let value = model.map_or_else(
        || {
            serde_json::json!({
                "type": "start",
                "id": run_id,
                "run": run_id
            })
        },
        |model| {
            serde_json::json!({
                "type": "start",
                "id": run_id,
                "run": run_id,
                "model": model
            })
        },
    );
    value.to_string()
}

fn socket_pong_frame() -> String {
    serde_json::json!({"type": "pong"}).to_string()
}

/// Records durable filesystem effects for a parsed socket request.
///
/// `send` appends a user message to `messages.jsonl`, appends a canonical
/// `start` event to `events.jsonl`, marks the session active, and records a
/// supplied chroot `cwd` when present. `cancel` appends a cancelled `done`
/// event and marks the session cancelled. `resume`, `ping`, and temp sessions
/// do not mutate durable session files.
pub fn record_socket_request_to_session(
    session_dir: &Path,
    request: &SocketRequest,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    match *request {
        SocketRequest::Send {
            ref id,
            ref session,
            scope,
            ref cwd,
            ref input,
        } => record_socket_send_to_session(session_dir, id, session, scope, cwd.as_deref(), input),
        SocketRequest::Cancel { ref id } => record_socket_cancel_to_session(session_dir, id),
        SocketRequest::Resume { .. } | SocketRequest::Ping => {
            Err(SocketSessionRecordError::UnsupportedRequest)
        }
    }
}

/// Records a durable socket `send` under `session_root/<session>/` and updates
/// the reserved session index files.
///
/// This is a filesystem helper for socket runtimes. It does not create
/// sessions, start models, or interpret provider state. The selected session
/// must already exist and have the v1 durable files.
pub fn record_indexed_socket_send_to_session(
    session_root: &Path,
    request: &SocketRequest,
) -> Result<SocketSessionRecord, IndexedSocketSessionRecordError> {
    let (session, scope, cwd) = match *request {
        SocketRequest::Send {
            ref session,
            scope,
            ref cwd,
            ..
        } => (session.as_str(), scope, cwd.as_deref()),
        SocketRequest::Resume { .. } | SocketRequest::Cancel { .. } | SocketRequest::Ping => {
            return Err(IndexedSocketSessionRecordError::Session(
                SocketSessionRecordError::UnsupportedRequest,
            ));
        }
    };
    if scope == SocketSessionScope::Temp {
        return Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::TempSessionNotDurable,
        ));
    }

    let session_dir = session_root.join(session);
    let record = record_socket_request_to_session(&session_dir, request)
        .map_err(IndexedSocketSessionRecordError::Session)?;
    let by_cwd_key = cwd.and_then(session_index_key_for_cwd);
    update_session_index(session_root, session, by_cwd_key.as_deref())
        .map_err(IndexedSocketSessionRecordError::Index)?;

    Ok(record)
}

/// Records a completed assistant response into durable session files.
///
/// This appends an assistant message to `messages.jsonl`, appends canonical
/// `message` and `done` events to `events.jsonl`, writes `latest.md`, and marks
/// the session `done`. Raw history remains append-only; `latest.md` is only the
/// latest inspectable convenience file.
pub fn record_assistant_response_to_session(
    session_dir: &Path,
    run_id: &str,
    content: &str,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    validate_socket_object_field("run", run_id)
        .map_err(|_error| SocketSessionRecordError::SessionMismatch)?;
    require_socket_session_files(session_dir)?;

    let message = serde_json::json!({
        "role": "assistant",
        "content": [
            {
                "type": "text",
                "text": content
            }
        ]
    })
    .to_string();
    let event = serde_json::json!({
        "type": "message",
        "run": run_id,
        "role": "assistant",
        "content": [
            {
                "type": "text",
                "text": content
            }
        ]
    })
    .to_string();
    let done = serde_json::json!({
        "type": "done",
        "run": run_id,
        "status": "ok"
    })
    .to_string();

    append_jsonl_line(&session_dir.join("messages.jsonl"), &message)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    append_jsonl_line(&session_dir.join("events.jsonl"), &event)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    append_jsonl_line(&session_dir.join("events.jsonl"), &done)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    atomic_replace_text(&session_dir.join("latest.md"), &format!("{content}\n"))
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    atomic_replace_text(&session_dir.join("state"), "done\n")
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    atomic_replace_text(&session_dir.join("updated_at"), &unix_timestamp_text())
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;

    Ok(SocketSessionRecord::new(vec![message], vec![event, done]))
}

/// Records a denied tool execution as durable session runtime history.
///
/// Denials are facts, not prompt text. Recording them in `events.jsonl` makes
/// policy failures inspectable without granting authority or executing the
/// requested tool.
pub fn record_tool_execution_denial_to_session(
    session_dir: &Path,
    run_id: &str,
    tool_name: &str,
    denial: ToolExecutionDenial,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    validate_socket_object_field("run", run_id)
        .map_err(|_error| SocketSessionRecordError::InvalidField("run"))?;
    if !is_object_name(tool_name) {
        return Err(SocketSessionRecordError::InvalidField("tool"));
    }
    require_socket_session_files(session_dir)?;

    let event = serde_json::json!({
        "type": "error",
        "run": run_id,
        "tool": tool_name,
        "code": denial.errno(),
        "message": "tool execution denied"
    })
    .to_string();
    let done = serde_json::json!({
        "type": "done",
        "run": run_id,
        "status": "error"
    })
    .to_string();

    append_jsonl_line(&session_dir.join("events.jsonl"), &event)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    append_jsonl_line(&session_dir.join("events.jsonl"), &done)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    atomic_replace_text(&session_dir.join("state"), "error\n")
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    atomic_replace_text(&session_dir.join("updated_at"), &unix_timestamp_text())
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;

    Ok(SocketSessionRecord::new(Vec::new(), vec![event, done]))
}

/// Records a successful tool execution result into durable session history.
///
/// Tool results are ordinary session messages and canonical `message` events.
/// The helper does not execute a tool and does not grant authority; callers
/// must run [`authorize_tool_execution`] before invoking the capability.
pub fn record_tool_execution_result_to_session(
    session_dir: &Path,
    run_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    content: &str,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    validate_socket_object_field("run", run_id)
        .map_err(|_error| SocketSessionRecordError::InvalidField("run"))?;
    validate_socket_object_field("tool_call_id", tool_call_id)
        .map_err(|_error| SocketSessionRecordError::InvalidField("tool_call_id"))?;
    if !is_object_name(tool_name) {
        return Err(SocketSessionRecordError::InvalidField("tool"));
    }
    if content.contains('\0') {
        return Err(SocketSessionRecordError::InvalidField("content"));
    }
    require_socket_session_files(session_dir)?;

    let content_part = serde_json::json!({
        "type": "tool_result",
        "tool_call_id": tool_call_id,
        "content": content
    });
    let message = serde_json::json!({
        "role": "tool",
        "name": tool_name,
        "content": [content_part]
    })
    .to_string();
    let event = serde_json::json!({
        "type": "message",
        "run": run_id,
        "role": "tool",
        "name": tool_name,
        "content": [content_part]
    })
    .to_string();

    append_jsonl_line(&session_dir.join("messages.jsonl"), &message)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    append_jsonl_line(&session_dir.join("events.jsonl"), &event)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    atomic_replace_text(&session_dir.join("updated_at"), &unix_timestamp_text())
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;

    Ok(SocketSessionRecord::new(vec![message], vec![event]))
}

/// Creates or replaces the parent-owned child handoff channel.
///
/// This writes only the documented `context/child/<child>/` files under the
/// parent session. It does not copy parent `messages.jsonl`, preserving the
/// child-context isolation rule.
pub fn record_child_handoff_to_parent_context(
    parent_session_dir: &Path,
    child_name: &str,
    child_agent: &str,
    child_session: &str,
    handoff: &str,
) -> Result<(), ChildContextRecordError> {
    validate_child_context_names(child_name, child_agent, child_session)?;
    if handoff.contains('\0') {
        return Err(ChildContextRecordError::InvalidText);
    }
    require_parent_session_context(parent_session_dir)?;

    let child_dir = parent_session_dir
        .join("context")
        .join("child")
        .join(child_name);
    fs::create_dir_all(child_dir.join("artifact"))
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    atomic_replace_text(&child_dir.join("agent"), &format!("{child_agent}\n"))
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    atomic_replace_text(&child_dir.join("session"), &format!("{child_session}\n"))
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    atomic_replace_text(
        &child_dir.join("status"),
        &format!("{}\n", ChildContextStatus::Pending.as_str()),
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    atomic_replace_text(
        &child_dir.join("handoff.md"),
        &ensure_trailing_newline(handoff),
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    write_text_file_if_absent(&child_dir.join("result.md"), "")
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    write_text_file_if_absent(&child_dir.join("refs.jsonl"), "")
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;

    Ok(())
}

/// Records a child result back into the parent session's child channel.
///
/// The result and refs are inspectable from the parent context pack through
/// `context/child/<child>/result.md` and `refs.jsonl`. This helper keeps the
/// child's full durable history in the child session, not in the parent pack.
pub fn record_child_result_to_parent_context(
    parent_session_dir: &Path,
    child_name: &str,
    status: ChildContextStatus,
    result: &str,
    refs_jsonl: &str,
) -> Result<(), ChildContextRecordError> {
    if !is_object_name(child_name) {
        return Err(ChildContextRecordError::InvalidChildName);
    }
    if matches!(
        status,
        ChildContextStatus::Pending | ChildContextStatus::Active
    ) {
        return Err(ChildContextRecordError::InvalidStatus);
    }
    if result.contains('\0') || refs_jsonl.contains('\0') {
        return Err(ChildContextRecordError::InvalidText);
    }
    if !inspect_context_jsonl(ContextJsonlKind::Refs, refs_jsonl).is_ok() {
        return Err(ChildContextRecordError::InvalidRefs);
    }

    let child_dir = parent_session_dir
        .join("context")
        .join("child")
        .join(child_name);
    require_child_context_files(&child_dir)?;
    atomic_replace_text(&child_dir.join("status"), &format!("{}\n", status.as_str()))
        .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    atomic_replace_text(
        &child_dir.join("result.md"),
        &ensure_trailing_newline(result),
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;
    atomic_replace_text(
        &child_dir.join("refs.jsonl"),
        &ensure_trailing_newline(refs_jsonl),
    )
    .map_err(|_error| ChildContextRecordError::CannotRecord)?;

    Ok(())
}

/// Derives the stable `session/index/by-cwd/<key>` file name for a chroot cwd.
#[must_use]
pub fn session_index_key_for_cwd(cwd: &str) -> Option<String> {
    if !is_stable_chroot_absolute_path(cwd) {
        return None;
    }
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in cwd.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    Some(format!("cwd-{hash:016x}"))
}

fn record_socket_send_to_session(
    session_dir: &Path,
    id: &str,
    session: &str,
    scope: SocketSessionScope,
    cwd: Option<&str>,
    input: &str,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    if scope == SocketSessionScope::Temp {
        return Err(SocketSessionRecordError::TempSessionNotDurable);
    }
    require_socket_session_name(session_dir, session)?;
    require_socket_session_files(session_dir)?;

    let message = serde_json::json!({
        "role": "user",
        "content": input
    })
    .to_string();
    let event = serde_json::json!({
        "type": "start",
        "id": id,
        "run": id
    })
    .to_string();

    append_jsonl_line(&session_dir.join("messages.jsonl"), &message)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    append_jsonl_line(&session_dir.join("events.jsonl"), &event)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    atomic_replace_text(&session_dir.join("state"), "active\n")
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    atomic_replace_text(&session_dir.join("updated_at"), &unix_timestamp_text())
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    if let Some(cwd) = cwd {
        atomic_replace_text(&session_dir.join("cwd"), &format!("{cwd}\n"))
            .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    }

    Ok(SocketSessionRecord::new(vec![message], vec![event]))
}

fn record_socket_cancel_to_session(
    session_dir: &Path,
    run_id: &str,
) -> Result<SocketSessionRecord, SocketSessionRecordError> {
    require_socket_session_files(session_dir)?;

    let event = serde_json::json!({
        "type": "done",
        "run": run_id,
        "status": "cancelled"
    })
    .to_string();
    append_jsonl_line(&session_dir.join("events.jsonl"), &event)
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    atomic_replace_text(&session_dir.join("state"), "cancelled\n")
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;
    atomic_replace_text(&session_dir.join("updated_at"), &unix_timestamp_text())
        .map_err(|_error| SocketSessionRecordError::CannotRecord)?;

    Ok(SocketSessionRecord::new(Vec::new(), vec![event]))
}

fn validate_child_context_names(
    child_name: &str,
    child_agent: &str,
    child_session: &str,
) -> Result<(), ChildContextRecordError> {
    if !is_object_name(child_name) {
        return Err(ChildContextRecordError::InvalidChildName);
    }
    if !is_object_name(child_agent) {
        return Err(ChildContextRecordError::InvalidAgentName);
    }
    if !is_object_name(child_session) {
        return Err(ChildContextRecordError::InvalidSessionName);
    }
    Ok(())
}

fn require_parent_session_context(
    parent_session_dir: &Path,
) -> Result<(), ChildContextRecordError> {
    for file in SESSION_REQUIRED_FILES {
        if !parent_session_dir.join(file).is_file() {
            return Err(ChildContextRecordError::MissingParentSession);
        }
    }
    if !parent_session_dir.join("context").is_dir() {
        return Err(ChildContextRecordError::MissingParentSession);
    }
    Ok(())
}

fn require_child_context_files(child_dir: &Path) -> Result<(), ChildContextRecordError> {
    for file in CHILD_RESULT_REQUIRED_FILES {
        if !child_dir.join(file).is_file() {
            return Err(ChildContextRecordError::MissingParentSession);
        }
    }
    for dir in CHILD_RESULT_REQUIRED_DIRS {
        if !child_dir.join(dir).is_dir() {
            return Err(ChildContextRecordError::MissingParentSession);
        }
    }
    Ok(())
}

fn write_text_file_if_absent(path: &Path, content: &str) -> std::io::Result<()> {
    if path.exists() {
        return if path.is_file() {
            Ok(())
        } else {
            Err(std::io::Error::other("path is not a regular file"))
        };
    }
    fs::write(path, content)
}

fn ensure_trailing_newline(content: &str) -> String {
    if content.is_empty() || content.ends_with('\n') {
        content.to_owned()
    } else {
        format!("{content}\n")
    }
}

fn require_socket_session_name(
    session_dir: &Path,
    session: &str,
) -> Result<(), SocketSessionRecordError> {
    if session_dir.file_name().and_then(|name| name.to_str()) == Some(session) {
        Ok(())
    } else {
        Err(SocketSessionRecordError::SessionMismatch)
    }
}

fn require_socket_session_files(session_dir: &Path) -> Result<(), SocketSessionRecordError> {
    for file in SESSION_REQUIRED_FILES {
        if !session_dir.join(file).is_file() {
            return Err(SocketSessionRecordError::MissingSessionFile(file));
        }
    }
    Ok(())
}

fn validate_socket_object_field(
    field: &'static str,
    value: &str,
) -> Result<(), SocketRequestError> {
    if is_object_name(value) {
        Ok(())
    } else {
        Err(SocketRequestError::InvalidField {
            field,
            value: value.to_owned(),
        })
    }
}

/// Derives an agent runtime view from the frozen v1 control files under
/// `ctx_root/agent/<agent_name>.d/`.
///
/// The returned environment always contains the runtime-owned `CTX_ROOT`,
/// `CTX_HOME`, `HOME`, and `CTX_PATH` values derived from the ABI controls.
/// Reserved keys present in `env` are ignored so text config cannot expand the
/// authority established by `path`, `mount`, and `policy`.
pub fn derive_agent_runtime_view(
    ctx_root: &Path,
    agent_name: &str,
) -> Result<AgentRuntimeView, AgentRuntimeViewError> {
    if !is_object_name(agent_name) {
        return Err(AgentRuntimeViewError::InvalidAgentName);
    }

    let control_dir = ctx_root.join("agent").join(format!("{agent_name}.d"));
    if !control_dir.is_dir() {
        return Err(AgentRuntimeViewError::MissingControlDirectory);
    }

    let owner = parse_agent_number_control(&control_dir, AgentControlKind::Owner, "owner")?;
    let uid = parse_agent_number_control(&control_dir, AgentControlKind::Uid, "uid")?;
    let gid = parse_agent_number_control(&control_dir, AgentControlKind::Gid, "gid")?;
    let groups = parse_agent_groups_control(&control_dir)?;
    let identity = AgentUnixIdentity::new(uid, gid, groups);

    let label = read_required_agent_control_value(&control_dir, "label")?;
    let policy_subject = policy_subject_from_label(&label)
        .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile("label".to_owned()))?
        .to_owned();

    let iso = parse_agent_vocab_value(&control_dir, AgentControlKind::Iso, "iso")?;
    let parent = parse_agent_parent_control(&control_dir)?;
    let lifecycle = ChildLifecycle::parse(&parse_agent_vocab_value(
        &control_dir,
        AgentControlKind::Life,
        "life",
    )?)
    .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("life".to_owned()))?;

    let root = parse_agent_absolute_path_control(&control_dir, "root")?;
    let cwd = parse_agent_absolute_path_control(&control_dir, "cwd")?;

    let raw_path = read_required_agent_control_value(&control_dir, "path")?;
    validate_agent_ctx_path(&raw_path)?;
    let tool_path = ToolPath::parse(&raw_path);

    let mount_content = read_required_agent_control(&control_dir, "mount")?;
    let mount_table = MountTable::parse(&mount_content)
        .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("mount".to_owned()))?;

    let model = read_required_agent_control_value(&control_dir, "model")?;
    if !is_model_name(&model) {
        return Err(AgentRuntimeViewError::InvalidControlFile(
            "model".to_owned(),
        ));
    }

    let policy_content = read_required_agent_control(&control_dir, "policy")?;
    let policy = PolicyV0::parse(&policy_content)
        .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("policy".to_owned()))?;

    let ctx_home = ctx_root.join("home").join(owner.to_string());
    let home = ctx_home.join("agent").join(agent_name);
    let env = derive_agent_runtime_env(ctx_root, &ctx_home, &home, &raw_path, &control_dir)?;

    Ok(AgentRuntimeView {
        agent_name: agent_name.to_owned(),
        control_dir,
        ctx_root: ctx_root.to_path_buf(),
        ctx_home,
        home,
        owner,
        identity,
        label,
        policy_subject,
        iso,
        parent,
        lifecycle,
        root,
        cwd,
        env,
        tool_path,
        mount_table,
        model,
        policy,
    })
}

fn parse_agent_number_control(
    control_dir: &Path,
    kind: AgentControlKind,
    file: &str,
) -> Result<u32, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, file)?;
    if !inspect_agent_control(kind, &content).is_ok() {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    required_single_agent_control_value(file, &content)?
        .parse::<u32>()
        .map_err(|_error| AgentRuntimeViewError::InvalidControlFile(file.to_owned()))
}

fn parse_agent_groups_control(control_dir: &Path) -> Result<Vec<u32>, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, "groups")?;
    if !inspect_agent_control(AgentControlKind::Groups, &content).is_ok() {
        return Err(AgentRuntimeViewError::InvalidControlFile(
            "groups".to_owned(),
        ));
    }
    content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            line.parse::<u32>()
                .map_err(|_error| AgentRuntimeViewError::InvalidControlFile("groups".to_owned()))
        })
        .collect()
}

fn parse_agent_vocab_value(
    control_dir: &Path,
    kind: AgentControlKind,
    file: &str,
) -> Result<String, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, file)?;
    if !inspect_agent_control(kind, &content).is_ok() {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    required_single_agent_control_value(file, &content)
}

fn parse_agent_parent_control(control_dir: &Path) -> Result<Option<String>, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, "parent")?;
    if !inspect_agent_control(AgentControlKind::Parent, &content).is_ok() {
        return Err(AgentRuntimeViewError::InvalidControlFile(
            "parent".to_owned(),
        ));
    }
    let value = optional_single_agent_control_value("parent", &content)?;
    Ok(value.filter(|parent| !parent.is_empty()))
}

fn parse_agent_absolute_path_control(
    control_dir: &Path,
    file: &str,
) -> Result<PathBuf, AgentRuntimeViewError> {
    let value = read_required_agent_control_value(control_dir, file)?;
    if !is_stable_chroot_absolute_path(&value) {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    Ok(PathBuf::from(value))
}

fn derive_agent_runtime_env(
    ctx_root: &Path,
    ctx_home: &Path,
    home: &Path,
    ctx_path: &str,
    control_dir: &Path,
) -> Result<Vec<(String, String)>, AgentRuntimeViewError> {
    let env_content = read_required_agent_control(control_dir, "env")?;
    let mut env = vec![
        ("CTX_ROOT".to_owned(), ctx_root.display().to_string()),
        ("CTX_HOME".to_owned(), ctx_home.display().to_string()),
        ("HOME".to_owned(), home.display().to_string()),
        ("CTX_PATH".to_owned(), ctx_path.to_owned()),
    ];
    for (key, value) in parse_agent_env_control(&env_content)? {
        if !matches!(key.as_str(), "CTX_ROOT" | "CTX_HOME" | "HOME" | "CTX_PATH") {
            env.push((key, value));
        }
    }
    Ok(env)
}

fn parse_agent_env_control(content: &str) -> Result<Vec<(String, String)>, AgentRuntimeViewError> {
    let mut env = Vec::new();
    for raw_line in content.lines().filter(|line| !line.trim().is_empty()) {
        let (key, value) = raw_line
            .split_once('=')
            .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile("env".to_owned()))?;
        if !is_valid_env_key(key) || value.contains('\0') {
            return Err(AgentRuntimeViewError::InvalidControlFile("env".to_owned()));
        }
        env.push((key.to_owned(), value.to_owned()));
    }
    Ok(env)
}

/// Resolves an API key with the stable priority: environment, system keychain,
/// then unconfigured.
pub fn resolve_api_key(
    env_name: &str,
    service: &str,
    account: &str,
) -> Result<Option<String>, ApiKeyResolutionError> {
    resolve_api_key_with(
        env_name,
        service,
        account,
        env_var_secret,
        system_keychain_secret,
    )
}

/// Testable core for API key resolution.
pub fn resolve_api_key_with<E, K>(
    env_name: &str,
    service: &str,
    account: &str,
    env_lookup: E,
    keychain_lookup: K,
) -> Result<Option<String>, ApiKeyResolutionError>
where
    E: FnOnce(&str) -> Result<String, std::env::VarError>,
    K: FnOnce(&str, &str) -> Result<Option<String>, ApiKeyResolutionError>,
{
    if !is_valid_env_key(env_name)
        || !is_valid_secret_lookup_part(service)
        || !is_valid_secret_lookup_part(account)
    {
        return Err(ApiKeyResolutionError::InvalidName);
    }
    match env_lookup(env_name) {
        Ok(value) if !value.trim().is_empty() => return Ok(Some(value)),
        Ok(_value) => {}
        Err(std::env::VarError::NotPresent) => {}
        Err(std::env::VarError::NotUnicode(_value)) => {
            return Err(ApiKeyResolutionError::InvalidName);
        }
    }
    keychain_lookup(service, account)
}

fn env_var_secret(name: &str) -> Result<String, std::env::VarError> {
    std::env::var(name)
}

fn system_keychain_secret(
    service: &str,
    account: &str,
) -> Result<Option<String>, ApiKeyResolutionError> {
    let entry = match keyring::Entry::new(service, account) {
        Ok(entry) => entry,
        Err(keyring::Error::NoDefaultStore) => return Ok(None),
        Err(_error) => return Err(ApiKeyResolutionError::KeychainUnavailable),
    };
    let secret = match entry.get_password() {
        Ok(secret) => secret,
        Err(keyring::Error::NoEntry) => return Ok(None),
        Err(_error) => return Err(ApiKeyResolutionError::KeychainUnavailable),
    };
    if secret.is_empty() {
        Ok(None)
    } else {
        Ok(Some(secret))
    }
}

fn is_valid_secret_lookup_part(value: &str) -> bool {
    !value.is_empty() && !value.contains('\0') && !value.contains('\n')
}

fn validate_agent_ctx_path(value: &str) -> Result<(), AgentRuntimeViewError> {
    if value
        .split(':')
        .filter(|component| !component.is_empty())
        .all(is_stable_chroot_absolute_path)
    {
        Ok(())
    } else {
        Err(AgentRuntimeViewError::InvalidControlFile("path".to_owned()))
    }
}

fn read_required_agent_control_value(
    control_dir: &Path,
    file: &str,
) -> Result<String, AgentRuntimeViewError> {
    let content = read_required_agent_control(control_dir, file)?;
    required_single_agent_control_value(file, &content)
}

fn read_required_agent_control(
    control_dir: &Path,
    file: &str,
) -> Result<String, AgentRuntimeViewError> {
    let path = control_dir.join(file);
    fs::read_to_string(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            AgentRuntimeViewError::MissingControlFile(file.to_owned())
        } else {
            AgentRuntimeViewError::CannotReadControl(file.to_owned())
        }
    })
}

fn required_single_agent_control_value(
    file: &str,
    content: &str,
) -> Result<String, AgentRuntimeViewError> {
    optional_single_agent_control_value(file, content)?
        .ok_or_else(|| AgentRuntimeViewError::InvalidControlFile(file.to_owned()))
}

fn optional_single_agent_control_value(
    file: &str,
    content: &str,
) -> Result<Option<String>, AgentRuntimeViewError> {
    let lines = content.lines().collect::<Vec<_>>();
    if lines.len() > 1 {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    let Some(raw_value) = lines.first() else {
        return Ok(None);
    };
    let value = raw_value.trim();
    if *raw_value != value || value.contains('\0') {
        return Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()));
    }
    if value.is_empty() {
        Ok(None)
    } else {
        Ok(Some(value.to_owned()))
    }
}

/// Returns executable metadata text for a model object.
pub fn model_exec_metadata(name: &str, control_dir: &Path) -> Result<String, FuseV1Error> {
    if !is_model_name(name) {
        return Err(FuseV1Error::InvalidPath);
    }
    let id = read_object_control_for_metadata(control_dir, "id")?;
    let driver_content = read_object_control_for_metadata(control_dir, "driver")?;
    let driver_routes =
        parse_model_driver_routes(&driver_content).map_err(|_error| FuseV1Error::InvalidContent)?;
    let driver = driver_routes
        .primary_driver_for(ModelDriverUseCase::Default)
        .unwrap_or("");
    let session = read_object_control_for_metadata(control_dir, "session")?;
    let status = read_object_control_for_metadata(control_dir, "status")?;
    let cap = read_object_control_for_metadata(control_dir, "cap")?;
    let description = model_metadata_description(name, driver);
    let model_type = model_metadata_type(driver);
    let owned_by = model_metadata_owner(name, driver);
    let context_length = model_metadata_context_length(name, driver);
    Ok(exec_metadata(&[
        ("object", "model".to_owned()),
        ("id", id),
        ("name", name.to_owned()),
        ("description", description.to_owned()),
        ("type", model_type.to_owned()),
        ("created_at", String::new()),
        ("owned_by", owned_by.to_owned()),
        ("context_length", context_length.to_string()),
        ("driver", driver.to_owned()),
        (
            "driver.default",
            driver_routes.route_value(ModelDriverUseCase::Default),
        ),
        (
            "driver.exec",
            driver_routes.route_value(ModelDriverUseCase::Exec),
        ),
        (
            "driver.socket",
            driver_routes.route_value(ModelDriverUseCase::Socket),
        ),
        (
            "driver.agent",
            driver_routes.route_value(ModelDriverUseCase::Agent),
        ),
        ("session", session),
        ("status", status),
        ("cap", cap.lines().collect::<Vec<_>>().join(",")),
    ]))
}

/// Returns executable metadata text for a tool object.
pub fn tool_exec_metadata(name: &str, control_dir: &Path) -> Result<String, FuseV1Error> {
    if !is_object_name(name) {
        return Err(FuseV1Error::InvalidPath);
    }
    let declared_name = read_object_control_for_metadata(control_dir, "name")
        .unwrap_or_else(|_error| name.to_owned());
    let description =
        read_object_control_for_metadata(control_dir, "description").unwrap_or_default();
    let cap = read_object_control_for_metadata(control_dir, "cap").unwrap_or_default();
    let status = read_object_control_for_metadata(control_dir, "status")
        .unwrap_or_else(|_error| "unknown".to_owned());
    Ok(exec_metadata(&[
        ("object", "tool".to_owned()),
        ("name", name.to_owned()),
        ("declared_name", declared_name),
        ("description", description),
        ("runner", "cortexfs-object-runner".to_owned()),
        ("status", status),
        ("cap", cap.lines().collect::<Vec<_>>().join(",")),
    ]))
}

/// Returns executable metadata text for an agent object.
pub fn agent_exec_metadata(name: &str, control_dir: &Path) -> Result<String, FuseV1Error> {
    if !is_object_name(name) {
        return Err(FuseV1Error::InvalidPath);
    }
    let owner = read_object_control_for_metadata(control_dir, "owner").unwrap_or_default();
    let uid = read_object_control_for_metadata(control_dir, "uid").unwrap_or_default();
    let gid = read_object_control_for_metadata(control_dir, "gid").unwrap_or_default();
    let label = read_object_control_for_metadata(control_dir, "label").unwrap_or_default();
    let model = read_object_control_for_metadata(control_dir, "model").unwrap_or_default();
    let status = read_object_control_for_metadata(control_dir, "status")
        .unwrap_or_else(|_error| "unknown".to_owned());
    let pid = read_object_control_for_metadata(control_dir, "pid").unwrap_or_default();
    Ok(exec_metadata(&[
        ("object", "agent".to_owned()),
        ("name", name.to_owned()),
        ("runner", "cortexfs-object-runner".to_owned()),
        ("owner", owner),
        ("uid", uid),
        ("gid", gid),
        ("label", label),
        ("model", model),
        ("status", status),
        ("pid", pid),
    ]))
}

fn object_exec_metadata(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
) -> Result<String, FuseV1Error> {
    match class {
        ObjectClass::Model => model_exec_metadata(name, control_dir),
        ObjectClass::Agent => agent_exec_metadata(name, control_dir),
        ObjectClass::Tool => tool_exec_metadata(name, control_dir),
    }
}

fn exec_metadata(fields: &[(&str, String)]) -> String {
    let mut output = format!("#!{CORTEXFS_OBJECT_RUNNER}\n");
    for field in fields {
        output.push_str("# cortexfs.");
        output.push_str(field.0);
        output.push('=');
        output.push_str(&field.1);
        output.push('\n');
    }
    output
}

fn model_metadata_description(name: &str, driver: &str) -> &'static str {
    if name == "debug/echo" && driver == "debug" {
        "Built-in debug echo model"
    } else {
        ""
    }
}

fn model_metadata_type(driver: &str) -> &str {
    if driver == "debug" { "debug" } else { "chat" }
}

fn model_metadata_owner(name: &str, driver: &str) -> &'static str {
    if name == "debug/echo" && driver == "debug" {
        "cortexfs"
    } else {
        ""
    }
}

fn model_metadata_context_length(_name: &str, _driver: &str) -> u64 {
    0
}

/// Runs the built-in debug echo model and writes canonical JSONL.
pub fn run_echo_model<I, S, W>(args: I, mut stdout: W) -> std::io::Result<()>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    W: Write,
{
    let mut input = args
        .into_iter()
        .map(|value| value.as_ref().to_owned())
        .collect::<Vec<_>>()
        .join(" ");
    if input.is_empty() {
        std::io::stdin().read_to_string(&mut input)?;
    }
    let run = std::env::var("CTX_RUN_ID").unwrap_or_else(|_error| "r1".to_owned());
    let text = serde_json::to_string(&input).unwrap_or_else(|_error| "\"\"".to_owned());
    stdout.write_all(
        format!(r#"{{"type":"start","run":"{run}","model":"debug/echo"}}"#).as_bytes(),
    )?;
    stdout.write_all(b"\n")?;
    stdout.write_all(format!(r#"{{"type":"delta","run":"{run}","text":{text}}}"#).as_bytes())?;
    stdout.write_all(b"\n")?;
    stdout.write_all(format!(r#"{{"type":"done","run":"{run}","status":"ok"}}"#).as_bytes())?;
    stdout.write_all(b"\n")
}

fn read_object_control_for_metadata(control_dir: &Path, file: &str) -> Result<String, FuseV1Error> {
    fs::read_to_string(control_dir.join(file))
        .map(|content| content.trim_end_matches('\n').to_owned())
        .map_err(|error| fuse_metadata_error(&error))
}

fn policy_subject_from_label(label: &str) -> Option<&str> {
    if is_object_name(label) {
        return Some(label);
    }
    let mut fields = label.split(':');
    let _user = fields.next()?;
    let _role = fields.next()?;
    let subject = fields.next()?;
    let _level = fields.next()?;
    if fields.next().is_none() && is_object_name(subject) {
        Some(subject)
    } else {
        None
    }
}

fn is_valid_env_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first == b'_' || first.is_ascii_alphabetic())
        && bytes.all(|byte| byte == b'_' || byte.is_ascii_alphanumeric())
}

/// Installs a v1 executable object wrapper plus required `.d` control files.
///
/// The wrapper is a small POSIX shell `exec` shim to an existing runtime,
/// script, or tool command. This helper does not start sockets, providers, or
/// supervisors; it only creates stable filesystem ABI entries that ordinary
/// runtimes can execute.
pub fn install_executable_object_wrapper(
    root: &Path,
    class: ObjectClass,
    name: &str,
    wrapper_target: &str,
    control_overrides: &[(&str, &str)],
) -> Result<ObjectBootstrap, ObjectBootstrapError> {
    if !is_object_name_for_class(class, name) {
        return Err(ObjectBootstrapError::InvalidObjectName);
    }
    if !is_valid_wrapper_target(wrapper_target) {
        return Err(ObjectBootstrapError::InvalidWrapperTarget);
    }
    validate_control_overrides(class, control_overrides)?;

    let class_dir = root.join(class.as_str());
    let control_dir = class_dir.join(format!("{name}.d"));
    fs::create_dir_all(&control_dir).map_err(|_error| ObjectBootstrapError::CannotCreate)?;

    let executable = class_dir.join(name);
    let wrapper = executable_wrapper_script(wrapper_target);
    atomic_replace_text(&executable, &wrapper)
        .map_err(|_error| ObjectBootstrapError::CannotRecord)?;
    set_executable_mode(&executable)?;

    for file in control_files_for(class) {
        let content = object_control_content(class, name, file, control_overrides)?;
        atomic_replace_text(&control_dir.join(file), &content)
            .map_err(|_error| ObjectBootstrapError::CannotRecord)?;
    }

    Ok(ObjectBootstrap::new(executable, control_dir))
}

fn validate_control_overrides(
    class: ObjectClass,
    control_overrides: &[(&str, &str)],
) -> Result<(), ObjectBootstrapError> {
    for (file, value) in control_overrides.iter().copied() {
        if !control_files_for(class).contains(&file) {
            return Err(ObjectBootstrapError::InvalidControlFile);
        }
        validate_object_control_content(class, file, &ensure_trailing_newline(value))?;
    }
    Ok(())
}

fn object_control_content(
    class: ObjectClass,
    object_name: &str,
    file: &str,
    control_overrides: &[(&str, &str)],
) -> Result<String, ObjectBootstrapError> {
    let value = control_overrides
        .iter()
        .copied()
        .find_map(|(override_file, value)| (override_file == file).then_some(value))
        .map_or_else(
            || default_object_control_value(class, object_name, file),
            ToOwned::to_owned,
        );
    let content = ensure_trailing_newline(&value);
    validate_object_control_content(class, file, &content)?;
    Ok(content)
}

fn validate_object_control_content(
    class: ObjectClass,
    file: &str,
    content: &str,
) -> Result<(), ObjectBootstrapError> {
    match class {
        ObjectClass::Model => validate_model_control_content(file, content),
        ObjectClass::Agent => validate_agent_bootstrap_control_content(file, content),
        ObjectClass::Tool => validate_tool_control_content(file, content),
    }
}

fn validate_model_control_content(file: &str, content: &str) -> Result<(), ObjectBootstrapError> {
    match file {
        "cap" if inspect_model_capabilities(content).is_ok() => Ok(()),
        "driver" if parse_model_driver_routes(content).is_ok() => Ok(()),
        "session" if matches!(content.trim(), "none" | "socket") => Ok(()),
        "cap" | "driver" | "session" => Err(ObjectBootstrapError::InvalidControlValue),
        _ if !content.contains('\0') => Ok(()),
        _ => Err(ObjectBootstrapError::InvalidControlValue),
    }
}

fn validate_agent_bootstrap_control_content(
    file: &str,
    content: &str,
) -> Result<(), ObjectBootstrapError> {
    if content.contains('\0') {
        return Err(ObjectBootstrapError::InvalidControlValue);
    }
    let Some(kind) = AgentControlKind::parse(file) else {
        return Ok(());
    };
    if inspect_agent_control(kind, content).is_ok() {
        Ok(())
    } else {
        Err(ObjectBootstrapError::InvalidControlValue)
    }
}

fn validate_tool_control_content(file: &str, content: &str) -> Result<(), ObjectBootstrapError> {
    match file {
        "schema" if inspect_tool_schema_json(content).is_ok() => Ok(()),
        "schema" => Err(ObjectBootstrapError::InvalidControlValue),
        _ if !content.contains('\0') => Ok(()),
        _ => Err(ObjectBootstrapError::InvalidControlValue),
    }
}

fn default_object_control_value(class: ObjectClass, object_name: &str, file: &str) -> String {
    match class {
        ObjectClass::Model => default_model_control_value(object_name, file),
        ObjectClass::Agent => default_agent_control_value(object_name, file),
        ObjectClass::Tool => default_tool_control_value(object_name, file),
    }
}

fn default_model_control_value(object_name: &str, file: &str) -> String {
    match file {
        "id" => object_name.to_owned(),
        "driver" => "rig".to_owned(),
        "cap" => "chat\nstream".to_owned(),
        "session" => "none".to_owned(),
        "status" => "idle".to_owned(),
        _ => String::new(),
    }
}

fn default_agent_control_value(object_name: &str, file: &str) -> String {
    match file {
        "owner" | "uid" | "gid" => "0".to_owned(),
        "label" => format!("user_u:agent_r:{object_name}_t:s0"),
        "iso" => "shared".to_owned(),
        "life" => "owned".to_owned(),
        "root" | "cwd" => "/".to_owned(),
        "env" => "CTX_ROOT=/ctx".to_owned(),
        "path" => "/ctx/tool".to_owned(),
        "mount" => "/ctx\t/ctx\tro\trbind,nosuid,nodev".to_owned(),
        "status" => "idle".to_owned(),
        "meta.json" => "{}".to_owned(),
        _ => String::new(),
    }
}

fn default_tool_control_value(object_name: &str, file: &str) -> String {
    match file {
        "name" => object_name.to_owned(),
        "schema" => "{\"type\":\"object\"}".to_owned(),
        "status" => "idle".to_owned(),
        _ => String::new(),
    }
}

fn is_valid_wrapper_target(value: &str) -> bool {
    !value.trim().is_empty() && !value.contains('\0') && !value.contains('\n')
}

fn executable_wrapper_script(wrapper_target: &str) -> String {
    format!(
        "#!/bin/sh\n# CortexFS generated object wrapper.\nexec {} \"$@\"\n",
        shell_single_quote(wrapper_target)
    )
}

/// Materializes the documented v1 reference tree under `root`.
///
/// This is a filesystem bootstrap helper for tests, local inspection, and
/// simple demos. It creates ABI-visible files, control directories, symlinks,
/// session skeletons, shared queue directories, and Unix socket path entries.
/// It does not start agents, models, MCP servers, providers, or a supervisor.
pub fn ensure_v1_reference_tree(root: &Path) -> Result<ReferenceTreeBootstrap, ReferenceTreeError> {
    create_reference_root(root)?;
    ensure_reference_bin(root)?;
    ensure_reference_agent(root, "coder")?;
    ensure_reference_agent(root, "reviewer")?;
    remove_deprecated_reference_placeholder_tools(root)?;
    ensure_reference_global_tools(root)?;
    ensure_reference_home(root)?;
    remove_deprecated_reference_home_tool_aliases(root)?;
    migrate_reference_legacy_session_meta_models(root)?;
    Ok(ReferenceTreeBootstrap::new(root.to_path_buf()))
}

fn create_reference_root(root: &Path) -> Result<(), ReferenceTreeError> {
    for entry in ROOT_ENTRIES {
        match *entry {
            "status" => write_reference_text(&root.join("status"), "ready\n")?,
            directory => create_reference_dir(&root.join(directory))?,
        }
    }
    Ok(())
}

fn ensure_reference_bin(root: &Path) -> Result<(), ReferenceTreeError> {
    let ctx = root.join("bin").join("ctx");
    write_reference_text(
        &ctx,
        "#!/bin/sh\n# CortexFS reference-tree ctx placeholder.\nexec ctx \"$@\"\n",
    )?;
    set_reference_executable(&ctx)
}

fn ensure_reference_agent(root: &Path, name: &str) -> Result<(), ReferenceTreeError> {
    install_executable_object_wrapper(root, ObjectClass::Agent, name, "/bin/false", &[])
        .map_err(ReferenceTreeError::Object)?;
    let control = root.join("agent").join(format!("{name}.d"));
    let label = format!("user_u:agent_r:{name}_t:s0\n");
    let home_root = format!("/ctx/home/1000/agent/{name}/root\n");
    let policy_subject = format!("{name}_t");
    let selected_model = DEBUG_ECHO_MODEL;
    let policy = format!(
        "allow {policy_subject} model:{selected_model} use\nallow {policy_subject} tool:fs.read execute\n"
    );
    let mount = format!(
        "/ctx\t/ctx\tro\trbind,nosuid,nodev\n/ctx/home/1000/agent/{name}\t/home/agent\trw\trbind,nosuid,nodev\n"
    );
    let overrides = [
        ("owner", "1000\n".to_owned()),
        ("uid", "1000\n".to_owned()),
        ("gid", "1000\n".to_owned()),
        ("groups", "1000\n".to_owned()),
        ("label", label),
        ("iso", "shared\n".to_owned()),
        ("parent", "\n".to_owned()),
        ("life", "owned\n".to_owned()),
        ("root", home_root),
        ("cwd", "/work\n".to_owned()),
        ("env", "CTX_ROOT=/ctx\n".to_owned()),
        ("path", "/ctx/tool:/ctx/home/1000/tool\n".to_owned()),
        ("mount", mount),
        ("model", format!("{selected_model}\n")),
        ("policy", policy),
        ("status", "idle\n".to_owned()),
        ("pid", "\n".to_owned()),
        ("log", "\n".to_owned()),
        ("meta.json", "{}\n".to_owned()),
    ];
    for (file, content) in overrides {
        write_reference_text(&control.join(file), &content)?;
    }
    write_reference_text(
        &root.join("agent").join(name),
        &reference_agent_stub_script(name),
    )?;
    set_reference_executable(&root.join("agent").join(name))?;
    ensure_reference_socket(&root.join("agent").join(format!("{name}.sock")))
}

fn reference_agent_stub_script(name: &str) -> String {
    format!(
        r#"#!/bin/sh
# CortexFS reference-tree agent stub. The selected model is a file ABI choice.
source_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
ctx_root="${{CTX_ROOT:-/ctx}}"
run="${{CTX_RUN_ID:-r1}}"
input="$*"
if [ -z "$input" ]; then
  input="$(cat)"
fi
model="$(tr -d '\n' < "$source_root/agent/{name}.d/model" 2>/dev/null || true)"
if [ -z "$model" ]; then
  model="debug/echo"
fi
if [ ! -x "$ctx_root/model/$model" ]; then
  printf '{{"type":"error","run":"%s","code":"ENOENT","message":"missing model"}}\n' "$run"
  printf '{{"type":"done","run":"%s","status":"error"}}\n' "$run"
  exit 1
fi
CTX_RUN_ID="$run" exec "$ctx_root/model/$model" "$input"
"#
    )
}

fn ensure_reference_global_tools(root: &Path) -> Result<(), ReferenceTreeError> {
    for tool in REFERENCE_GLOBAL_TOOLS {
        install_executable_object_wrapper(
            root,
            ObjectClass::Tool,
            tool.name,
            "/bin/false",
            &[
                ("name", tool.name),
                ("description", tool.description),
                ("schema", tool.schema),
                ("cap", tool.cap),
                ("policy", tool.policy),
                ("status", "idle"),
                ("log", ""),
            ],
        )
        .map_err(ReferenceTreeError::Object)?;
        if let Some(script) = reference_tool_stub_script(tool.name) {
            write_reference_text(&root.join("tool").join(tool.name), script)?;
            set_reference_executable(&root.join("tool").join(tool.name))?;
        }
    }
    Ok(())
}

fn remove_deprecated_reference_placeholder_tools(root: &Path) -> Result<(), ReferenceTreeError> {
    for tool in DEPRECATED_REFERENCE_PLACEHOLDER_TOOLS {
        remove_deprecated_reference_placeholder_tool(root, tool)?;
    }
    Ok(())
}

fn remove_deprecated_reference_placeholder_tool(
    root: &Path,
    name: &str,
) -> Result<(), ReferenceTreeError> {
    let executable = root.join("tool").join(name);
    let control_dir = root.join("tool").join(format!("{name}.d"));
    if !executable.exists() && !control_dir.exists() {
        return Ok(());
    }
    if !is_deprecated_reference_placeholder_tool(&executable, &control_dir) {
        return Ok(());
    }
    match fs::remove_file(&executable) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_error) => return Err(ReferenceTreeError::CannotRemove),
    }
    match fs::remove_dir_all(&control_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_error) => return Err(ReferenceTreeError::CannotRemove),
    }
    Ok(())
}

fn is_deprecated_reference_placeholder_tool(executable: &Path, control_dir: &Path) -> bool {
    let Ok(wrapper) = fs::read_to_string(executable) else {
        return false;
    };
    let Ok(description) = fs::read_to_string(control_dir.join("description")) else {
        return false;
    };
    wrapper == executable_wrapper_script("/bin/false")
        && description.trim_end_matches('\n') == "CortexFS reference-tree tool"
}

struct ReferenceToolSpec {
    name: &'static str,
    description: &'static str,
    schema: &'static str,
    cap: &'static str,
    policy: &'static str,
}

const REFERENCE_GLOBAL_TOOLS: &[ReferenceToolSpec] = &[
    ReferenceToolSpec {
        name: "fs.read",
        description: "Read a UTF-8 text file from the agent-visible filesystem.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.read input",
  "description": "Read one UTF-8 text file visible to the tool process.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path"],
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to a UTF-8 text file visible to the tool process."
    }
  }
}"#,
        cap: "fs.read",
        policy: "allow coder_t tool:fs.read execute\nallow reviewer_t tool:fs.read execute",
    },
    ReferenceToolSpec {
        name: "fs.write",
        description: "Write UTF-8 text to a file path visible to the tool process.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "fs.write input",
  "description": "Write UTF-8 text to one path visible to the tool process.",
  "type": "object",
  "additionalProperties": false,
  "required": ["path", "content"],
  "properties": {
    "path": {
      "type": "string",
      "description": "Path to write."
    },
    "content": {
      "type": "string",
      "description": "UTF-8 content to write."
    }
  }
}"#,
        cap: "fs.write",
        policy: "",
    },
    ReferenceToolSpec {
        name: "shell.exec",
        description: "Run a shell command in the tool process environment and return stdout/stderr.",
        schema: r#"{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "shell.exec input",
  "description": "Run one shell command in the tool process environment.",
  "type": "object",
  "additionalProperties": false,
  "required": ["cmd"],
  "properties": {
    "cmd": {
      "type": "string",
      "description": "Command line passed to sh -c."
    }
  }
}"#,
        cap: "shell.exec",
        policy: "",
    },
];

const DEPRECATED_REFERENCE_PLACEHOLDER_TOOLS: &[&str] = &[
    "mcp.github.search_issues",
    "agent.create",
    "agent.start",
    "agent.stop",
];

fn reference_tool_stub_script(name: &str) -> Option<&'static str> {
    match name {
        "fs.read" => Some(reference_fs_read_stub_script()),
        "fs.write" => Some(reference_fs_write_stub_script()),
        "shell.exec" => Some(reference_shell_exec_stub_script()),
        _ => None,
    }
}

fn reference_fs_read_stub_script() -> &'static str {
    r#"#!/bin/sh
# CortexFS reference-tree fs.read stub.
run="$CTX_RUN_ID"
if [ -z "$run" ]; then
  run="r1"
fi
input="$*"
if [ -z "$input" ]; then
  input="$(cat)"
fi
path="$(printf '%s' "$input" | sed -n 's/.*"path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
if [ -z "$path" ]; then
  path="$input"
fi
printf '{"type":"start","run":"%s","tool":"fs.read"}\n' "$run"
if [ ! -f "$path" ]; then
  printf '{"type":"error","run":"%s","code":"ENOENT","message":"file not found"}\n' "$run"
  printf '{"type":"done","run":"%s","status":"error"}\n' "$run"
  exit 2
fi
content="$(cat "$path")"
json_text="$(printf '%s' "$content" | sed 's/\\/\\\\/g; s/"/\\"/g')"
printf '{"type":"message","run":"%s","role":"tool","content":[{"type":"text","text":"%s"}]}\n' "$run" "$json_text"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$run"
"#
}

fn reference_fs_write_stub_script() -> &'static str {
    r#"#!/bin/sh
# CortexFS reference-tree fs.write stub.
run="$CTX_RUN_ID"
if [ -z "$run" ]; then
  run="r1"
fi
input="$*"
if [ -z "$input" ]; then
  input="$(cat)"
fi
path="$(printf '%s' "$input" | sed -n 's/.*"path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
content="$(printf '%s' "$input" | sed -n 's/.*"content"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
printf '{"type":"start","run":"%s","tool":"fs.write"}\n' "$run"
if [ -z "$path" ]; then
  printf '{"type":"error","run":"%s","code":"EINVAL","message":"missing path"}\n' "$run"
  printf '{"type":"done","run":"%s","status":"error"}\n' "$run"
  exit 2
fi
if ! printf '%s' "$content" > "$path"; then
  printf '{"type":"error","run":"%s","code":"EACCES","message":"write failed"}\n' "$run"
  printf '{"type":"done","run":"%s","status":"error"}\n' "$run"
  exit 13
fi
printf '{"type":"message","run":"%s","role":"tool","content":[{"type":"text","text":"written"}]}\n' "$run"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$run"
"#
}

fn reference_shell_exec_stub_script() -> &'static str {
    r#"#!/bin/sh
# CortexFS reference-tree shell.exec stub.
run="$CTX_RUN_ID"
if [ -z "$run" ]; then
  run="r1"
fi
input="$*"
if [ -z "$input" ]; then
  input="$(cat)"
fi
cmd="$(printf '%s' "$input" | sed -n 's/.*"cmd"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
if [ -z "$cmd" ]; then
  cmd="$input"
fi
printf '{"type":"start","run":"%s","tool":"shell.exec"}\n' "$run"
output="$(sh -c "$cmd" 2>&1)"
status="$?"
json_text="$(printf '%s' "$output" | sed 's/\\/\\\\/g; s/"/\\"/g')"
printf '{"type":"message","run":"%s","role":"tool","content":[{"type":"text","text":"%s"}]}\n' "$run" "$json_text"
if [ "$status" -eq 0 ]; then
  printf '{"type":"done","run":"%s","status":"ok"}\n' "$run"
else
  printf '{"type":"error","run":"%s","code":"EIO","message":"command failed"}\n' "$run"
  printf '{"type":"done","run":"%s","status":"error"}\n' "$run"
  exit 1
fi
"#
}

fn ensure_reference_home(root: &Path) -> Result<(), ReferenceTreeError> {
    let agent_root = root.join("home").join("1000").join("agent").join("coder");
    create_reference_dir(&agent_root.join("root"))?;
    create_reference_dir(&agent_root.join("session").join("index").join("by-cwd"))?;
    create_reference_dir(&agent_root.join("data"))?;
    create_reference_dir(&agent_root.join("cache"))?;
    create_reference_dir(&agent_root.join("log"))?;
    create_reference_dir(&root.join("home").join("1000").join("tool"))?;
    create_reference_dir(&root.join("home").join("1000").join("model"))?;

    ensure_reference_model_alias(
        &root.join("home").join("1000").join("model").join("coder"),
        Path::new("/ctx/model/debug/echo"),
    )
}

fn remove_deprecated_reference_home_tool_aliases(root: &Path) -> Result<(), ReferenceTreeError> {
    let alias = root.join("home").join("1000").join("tool").join("fs.read");
    match fs::read_link(&alias) {
        Ok(target) if target == Path::new("/ctx/tool/fs.read") => {
            fs::remove_file(alias).map_err(|_error| ReferenceTreeError::CannotUnlink)
        }
        Ok(_) | Err(_) => Ok(()),
    }
}

fn migrate_reference_legacy_session_meta_models(root: &Path) -> Result<(), ReferenceTreeError> {
    let mut meta_paths = Vec::new();
    collect_reference_agent_session_meta_paths(&root.join("home"), &mut meta_paths)?;
    collect_reference_shared_agent_session_meta_paths(&root.join("shared"), &mut meta_paths)?;
    for meta_path in meta_paths {
        migrate_reference_session_meta_model(&meta_path)?;
    }
    Ok(())
}

fn collect_reference_agent_session_meta_paths(
    home_root: &Path,
    meta_paths: &mut Vec<PathBuf>,
) -> Result<(), ReferenceTreeError> {
    let Ok(users) = fs::read_dir(home_root) else {
        return Ok(());
    };
    for user in users {
        let user = user.map_err(|_error| ReferenceTreeError::CannotCreate)?;
        if user
            .file_type()
            .map_err(|_error| ReferenceTreeError::CannotCreate)?
            .is_dir()
        {
            collect_reference_session_meta_paths(&user.path().join("agent"), meta_paths)?;
        }
    }
    Ok(())
}

fn collect_reference_shared_agent_session_meta_paths(
    shared_root: &Path,
    meta_paths: &mut Vec<PathBuf>,
) -> Result<(), ReferenceTreeError> {
    let Ok(spaces) = fs::read_dir(shared_root) else {
        return Ok(());
    };
    for space in spaces {
        let space = space.map_err(|_error| ReferenceTreeError::CannotCreate)?;
        if space
            .file_type()
            .map_err(|_error| ReferenceTreeError::CannotCreate)?
            .is_dir()
        {
            collect_reference_session_meta_paths(&space.path().join("agent"), meta_paths)?;
        }
    }
    Ok(())
}

fn collect_reference_session_meta_paths(
    agent_root: &Path,
    meta_paths: &mut Vec<PathBuf>,
) -> Result<(), ReferenceTreeError> {
    let Ok(agents) = fs::read_dir(agent_root) else {
        return Ok(());
    };
    for agent in agents {
        let agent = agent.map_err(|_error| ReferenceTreeError::CannotCreate)?;
        if !agent
            .file_type()
            .map_err(|_error| ReferenceTreeError::CannotCreate)?
            .is_dir()
        {
            continue;
        }
        let session_root = agent.path().join("session");
        let Ok(sessions) = fs::read_dir(session_root) else {
            continue;
        };
        for session in sessions {
            let session = session.map_err(|_error| ReferenceTreeError::CannotCreate)?;
            if session
                .file_type()
                .map_err(|_error| ReferenceTreeError::CannotCreate)?
                .is_dir()
            {
                let meta_path = session.path().join("meta.json");
                if meta_path.is_file() {
                    meta_paths.push(meta_path);
                }
            }
        }
    }
    Ok(())
}

fn migrate_reference_session_meta_model(meta_path: &Path) -> Result<(), ReferenceTreeError> {
    let content =
        fs::read_to_string(meta_path).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let Ok(mut value) = serde_json::from_str::<Value>(&content) else {
        return Ok(());
    };
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };
    let Some(model) = object.get("model").and_then(Value::as_str) else {
        return Ok(());
    };
    if is_model_name(model) || !is_object_name(model) {
        return Ok(());
    }

    object.insert("model".to_owned(), serde_json::json!("debug/echo"));
    let content =
        serde_json::to_string(&value).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    atomic_replace_text(meta_path, &format!("{content}\n"))
        .map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn create_reference_dir(path: &Path) -> Result<(), ReferenceTreeError> {
    fs::create_dir_all(path).map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn write_reference_text(path: &Path, content: &str) -> Result<(), ReferenceTreeError> {
    if let Some(parent) = path.parent() {
        create_reference_dir(parent)?;
    }
    atomic_replace_text(path, content).map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn set_reference_executable(path: &Path) -> Result<(), ReferenceTreeError> {
    let metadata = fs::metadata(path).map_err(|_error| ReferenceTreeError::CannotCreate)?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(|_error| ReferenceTreeError::CannotCreate)
}

fn ensure_reference_socket(path: &Path) -> Result<(), ReferenceTreeError> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        let file_type = metadata.file_type();
        if file_type.is_socket() {
            return set_reference_socket_permissions(path);
        }
        if file_type.is_symlink() {
            match fs::metadata(path) {
                Ok(target) if target.file_type().is_socket() => {
                    return set_reference_socket_permissions(path);
                }
                Ok(_target) => return Err(ReferenceTreeError::CannotSocket),
                Err(_error) => {
                    fs::remove_file(path).map_err(|_error| ReferenceTreeError::CannotSocket)?;
                }
            }
        } else {
            return Err(ReferenceTreeError::CannotSocket);
        }
    }
    if let Some(parent) = path.parent() {
        create_reference_dir(parent)?;
    }
    UnixListener::bind(path).map_err(|_error| ReferenceTreeError::CannotSocket)?;
    set_reference_socket_permissions(path)
}

fn set_reference_socket_permissions(path: &Path) -> Result<(), ReferenceTreeError> {
    let metadata = fs::metadata(path).map_err(|_error| ReferenceTreeError::CannotSocket)?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o777);
    fs::set_permissions(path, permissions).map_err(|_error| ReferenceTreeError::CannotSocket)
}

fn ensure_reference_model_alias(path: &Path, target: &Path) -> Result<(), ReferenceTreeError> {
    if let Ok(existing) = fs::read_link(path) {
        if existing == target || is_valid_ctx_model_symlink(&existing) {
            return Ok(());
        }
        if is_legacy_ctx_model_symlink(&existing) {
            fs::remove_file(path).map_err(|_error| ReferenceTreeError::CannotLink)?;
        } else {
            return Err(ReferenceTreeError::CannotLink);
        }
    } else if path.exists() {
        return Err(ReferenceTreeError::CannotLink);
    }
    if let Some(parent) = path.parent() {
        create_reference_dir(parent)?;
    }
    symlink(target, path).map_err(|_error| ReferenceTreeError::CannotLink)
}

fn is_valid_ctx_model_symlink(target: &Path) -> bool {
    let Some(target) = target.to_str() else {
        return false;
    };
    let Some(model) = target.strip_prefix("/ctx/model/") else {
        return false;
    };
    is_model_name(model)
}

fn is_legacy_ctx_model_symlink(target: &Path) -> bool {
    let Some(target) = target.to_str() else {
        return false;
    };
    let Some(model) = target.strip_prefix("/ctx/model/") else {
        return false;
    };
    is_object_name(model)
}

fn resolve_fuse_abi_path(root: &Path, abi_path: &str) -> Result<PathBuf, FuseV1Error> {
    let normalized = normalize_fuse_abi_path(abi_path)?;
    let mut resolved = root.to_path_buf();
    for component in Path::new(&normalized).components() {
        match component {
            std::path::Component::Normal(part) => resolved.push(part),
            std::path::Component::CurDir => {}
            std::path::Component::RootDir
            | std::path::Component::ParentDir
            | std::path::Component::Prefix(_) => return Err(FuseV1Error::InvalidPath),
        }
    }
    Ok(resolved)
}

fn normalize_fuse_abi_path(abi_path: &str) -> Result<String, FuseV1Error> {
    if abi_path.contains('\0') {
        return Err(FuseV1Error::InvalidPath);
    }
    let trimmed = abi_path.trim_start_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Ok(String::new());
    }
    let mut parts = Vec::new();
    for component in Path::new(trimmed).components() {
        match component {
            std::path::Component::Normal(part) => {
                parts.push(part.to_str().ok_or(FuseV1Error::InvalidPath)?);
            }
            std::path::Component::CurDir => {}
            std::path::Component::RootDir
            | std::path::Component::ParentDir
            | std::path::Component::Prefix(_) => return Err(FuseV1Error::InvalidPath),
        }
    }
    Ok(parts.join("/"))
}

fn model_exec_name(abi_path: &str) -> Option<&str> {
    let model = abi_path.strip_prefix("model/")?;
    is_model_name(model).then_some(model)
}

fn read_bytes_at(content: &[u8], offset: u64, size: usize) -> Result<Vec<u8>, FuseV1Error> {
    let start = usize::try_from(offset).map_err(|_error| FuseV1Error::Io)?;
    if start >= content.len() {
        return Ok(Vec::new());
    }
    let end = start.saturating_add(size).min(content.len());
    content
        .get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or(FuseV1Error::Io)
}

fn fuse_join_child_path(parent: &str, name: &str) -> Result<String, FuseV1Error> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\0') {
        return Err(FuseV1Error::InvalidPath);
    }
    if parent.is_empty() {
        normalize_fuse_abi_path(name)
    } else {
        normalize_fuse_abi_path(&format!("{parent}/{name}"))
    }
}

fn fuse_v1_inode_for_path(abi_path: &str) -> u64 {
    if abi_path.is_empty() {
        return FUSE_V1_ROOT_INODE;
    }
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in abi_path.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let inode = hash & 0x7fff_ffff_ffff_ffff;
    if inode == 0 || inode == FUSE_V1_ROOT_INODE {
        FUSE_V1_ROOT_INODE + 1
    } else {
        inode
    }
}

fn fuse_file_type(file_type: fs::FileType) -> FuseV1FileType {
    if file_type.is_dir() {
        FuseV1FileType::Directory
    } else if file_type.is_symlink() {
        FuseV1FileType::Symlink
    } else if file_type.is_socket() {
        FuseV1FileType::Socket
    } else if file_type.is_char_device() || file_type.is_block_device() || file_type.is_fifo() {
        FuseV1FileType::Other
    } else {
        FuseV1FileType::Regular
    }
}

fn fuse_metadata_error(error: &std::io::Error) -> FuseV1Error {
    if error.kind() == std::io::ErrorKind::NotFound {
        FuseV1Error::NotFound
    } else {
        FuseV1Error::Io
    }
}

fn is_fuse_v1_writable_control_path(abi_path: &str) -> bool {
    matches!(
        classify_abi_path(abi_path),
        "ctx.model.control" | "ctx.agent.control" | "ctx.tool.control" | "ctx.shared.tool.control"
    )
}

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
    let metadata = fs::metadata(path).map_err(|_error| ObjectBootstrapError::CannotChmod)?;
    let mut permissions = metadata.permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).map_err(|_error| ObjectBootstrapError::CannotChmod)
}

/// Inspects a model, agent, or tool object triple under a `CortexFS` root.
#[must_use]
pub fn inspect_object_layout(root: &Path, class: ObjectClass, name: &str) -> ObjectLayoutReport {
    let mut issues = Vec::new();
    if !is_object_name_for_class(class, name) {
        issues.push(ObjectLayoutIssue::MissingExecutable(format!(
            "{}/{}",
            class.as_str(),
            name
        )));
        return ObjectLayoutReport::new(issues);
    }
    if class == ObjectClass::Model && name == DEBUG_ECHO_MODEL {
        return ObjectLayoutReport::new(issues);
    }

    let exec_label = format!("{}/{name}", class.as_str());
    let exec_path = root.join(class.as_str()).join(name);
    require_executable_file(&exec_path, &exec_label, &mut issues);

    let control_label = format!("{}/{name}.d", class.as_str());
    let control_dir = root.join(class.as_str()).join(format!("{name}.d"));
    require_object_control_dir(&control_dir, &control_label, &mut issues);
    for file in control_files_for(class) {
        let label = format!("{control_label}/{file}");
        require_object_control_file(&control_dir.join(file), &label, &mut issues);
    }

    inspect_object_socket(root, class, name, &control_dir, &mut issues);
    inspect_model_capability_control(class, name, &control_dir, &mut issues);
    inspect_model_driver_control(class, name, &control_dir, &mut issues);
    inspect_tool_schema_control(class, name, &control_dir, &mut issues);
    inspect_agent_control_files(class, name, &control_dir, &mut issues);
    ObjectLayoutReport::new(issues)
}

fn control_files_for(class: ObjectClass) -> &'static [&'static str] {
    match class {
        ObjectClass::Model => MODEL_CONTROL_FILES,
        ObjectClass::Agent => AGENT_CONTROL_FILES,
        ObjectClass::Tool => TOOL_CONTROL_FILES,
    }
}

fn inspect_object_socket(
    root: &Path,
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<ObjectLayoutIssue>,
) {
    let socket_label = format!("{}/{name}.sock", class.as_str());
    let socket_path = root.join(class.as_str()).join(format!("{name}.sock"));
    match class {
        ObjectClass::Agent => require_unix_socket(&socket_path, &socket_label, true, issues),
        ObjectClass::Model => {
            let session_label = format!("{}/{name}.d/session", class.as_str());
            inspect_model_socket(
                &socket_path,
                &socket_label,
                &session_label,
                control_dir,
                issues,
            );
        }
        ObjectClass::Tool => require_unix_socket(&socket_path, &socket_label, false, issues),
    }
}

fn inspect_model_socket(
    socket_path: &Path,
    socket_label: &str,
    session_label: &str,
    control_dir: &Path,
    issues: &mut Vec<ObjectLayoutIssue>,
) {
    let session_path = control_dir.join("session");
    let Ok(content) = fs::read_to_string(&session_path) else {
        return;
    };
    let value = content.trim();
    match value {
        "socket" => require_unix_socket(socket_path, socket_label, true, issues),
        "none" => require_unix_socket(socket_path, socket_label, false, issues),
        _ => issues.push(ObjectLayoutIssue::InvalidControlValue {
            path: session_label.to_owned(),
            value: value.to_owned(),
        }),
    }
}

fn inspect_model_capability_control(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<ObjectLayoutIssue>,
) {
    if class != ObjectClass::Model {
        return;
    }

    let Ok(content) = fs::read_to_string(control_dir.join("cap")) else {
        return;
    };
    for issue in inspect_model_capabilities(&content).issues() {
        let value = match *issue {
            ModelCapabilityIssue::ProviderPrivate { ref capability, .. }
            | ModelCapabilityIssue::Unknown { ref capability, .. } => capability,
        };
        issues.push(ObjectLayoutIssue::InvalidControlValue {
            path: format!("model/{name}.d/cap"),
            value: value.to_owned(),
        });
    }
}

fn inspect_model_driver_control(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<ObjectLayoutIssue>,
) {
    if class != ObjectClass::Model {
        return;
    }

    let Ok(content) = fs::read_to_string(control_dir.join("driver")) else {
        return;
    };
    if let Err(error) = parse_model_driver_routes(&content) {
        issues.push(ObjectLayoutIssue::InvalidControlValue {
            path: format!("model/{name}.d/driver"),
            value: model_driver_route_error_value(&error),
        });
    }
}

fn model_driver_route_error_value(error: &ModelDriverRouteError) -> String {
    match *error {
        ModelDriverRouteError::Empty => "empty".to_owned(),
        ModelDriverRouteError::MissingEquals { line } => format!("line {line} missing ="),
        ModelDriverRouteError::UnknownUseCase { line, ref value } => {
            format!("line {line} unknown use case {value}")
        }
        ModelDriverRouteError::DuplicateUseCase { line, ref value } => {
            format!("line {line} duplicate use case {value}")
        }
        ModelDriverRouteError::EmptyDriver { line } => format!("line {line} empty driver"),
        ModelDriverRouteError::InvalidDriverName { line, ref value } => {
            format!("line {line} invalid driver {value}")
        }
    }
}

fn inspect_tool_schema_control(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<ObjectLayoutIssue>,
) {
    if class != ObjectClass::Tool {
        return;
    }

    let Ok(content) = fs::read_to_string(control_dir.join("schema")) else {
        return;
    };
    for issue in inspect_tool_schema_json(&content).issues() {
        issues.push(ObjectLayoutIssue::InvalidControlValue {
            path: format!("tool/{name}.d/schema"),
            value: tool_schema_issue_value(issue).to_owned(),
        });
    }
}

fn tool_schema_issue_value(issue: &ToolSchemaIssue) -> &str {
    match *issue {
        ToolSchemaIssue::AuthorityField(ref field) => field,
        ToolSchemaIssue::InvalidJson
        | ToolSchemaIssue::NotObject
        | ToolSchemaIssue::InvalidSchema => "",
    }
}

fn inspect_agent_control_files(
    class: ObjectClass,
    name: &str,
    control_dir: &Path,
    issues: &mut Vec<ObjectLayoutIssue>,
) {
    if class != ObjectClass::Agent {
        return;
    }

    for file in AGENT_CONTROL_FILES {
        let Some(kind) = AgentControlKind::parse(file) else {
            continue;
        };
        let Ok(content) = fs::read_to_string(control_dir.join(file)) else {
            continue;
        };
        for issue in inspect_agent_control(kind, &content).issues() {
            issues.push(ObjectLayoutIssue::InvalidControlValue {
                path: format!("agent/{name}.d/{file}"),
                value: agent_control_issue_value(issue).to_owned(),
            });
        }
    }
}

fn agent_control_issue_value(issue: &AgentControlIssue) -> &str {
    match *issue {
        AgentControlIssue::InvalidNumber { ref value, .. }
        | AgentControlIssue::InvalidValue { ref value, .. } => value,
        AgentControlIssue::EmptyValue | AgentControlIssue::MultipleValues { .. } => "",
    }
}

fn require_executable_file(path: &Path, label: &str, issues: &mut Vec<ObjectLayoutIssue>) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.permissions().mode() & 0o111 != 0 => {}
        Ok(_metadata) => issues.push(ObjectLayoutIssue::NotExecutable(label.to_owned())),
        Err(_error) => issues.push(ObjectLayoutIssue::MissingExecutable(label.to_owned())),
    }
}

fn require_object_control_dir(path: &Path, label: &str, issues: &mut Vec<ObjectLayoutIssue>) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_metadata) => issues.push(ObjectLayoutIssue::NotControlDirectory(label.to_owned())),
        Err(_error) => issues.push(ObjectLayoutIssue::MissingControlDirectory(label.to_owned())),
    }
}

fn require_object_control_file(path: &Path, label: &str, issues: &mut Vec<ObjectLayoutIssue>) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_metadata) => issues.push(ObjectLayoutIssue::NotControlFile(label.to_owned())),
        Err(_error) => issues.push(ObjectLayoutIssue::MissingControlFile(label.to_owned())),
    }
}

fn require_unix_socket(
    path: &Path,
    label: &str,
    required: bool,
    issues: &mut Vec<ObjectLayoutIssue>,
) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.file_type().is_socket() => {}
        Ok(_metadata) => issues.push(ObjectLayoutIssue::NotSocket(label.to_owned())),
        Err(_error) if required => issues.push(ObjectLayoutIssue::MissingSocket(label.to_owned())),
        Err(_error) => {}
    }
}

/// Inspects a durable session directory for the v1 transparency/context layout.
#[must_use]
pub fn inspect_session_layout(session_dir: &Path) -> SessionLayoutReport {
    let mut issues = Vec::new();
    require_directory(session_dir, ".", &mut issues);
    for file in SESSION_REQUIRED_FILES {
        require_file(&session_dir.join(file), file, &mut issues);
    }
    inspect_session_control_files(session_dir, &mut issues);

    let context = session_dir.join("context");
    require_directory(&context, "context", &mut issues);
    for file in CONTEXT_REQUIRED_FILES {
        let label = format!("context/{file}");
        require_file(&context.join(file), &label, &mut issues);
    }
    for dir in CONTEXT_REQUIRED_DIRS {
        let label = format!("context/{dir}");
        require_directory(&context.join(dir), &label, &mut issues);
    }
    inspect_child_result_dirs(&context.join("child"), &mut issues);

    SessionLayoutReport::new(issues)
}

fn inspect_session_control_files(session_dir: &Path, issues: &mut Vec<SessionLayoutIssue>) {
    for file in SESSION_REQUIRED_FILES {
        let Some(kind) = SessionControlKind::parse(file) else {
            continue;
        };
        let Ok(content) = fs::read_to_string(session_dir.join(file)) else {
            continue;
        };
        for issue in inspect_session_control(kind, &content).issues() {
            issues.push(SessionLayoutIssue::InvalidFileValue {
                path: (*file).to_owned(),
                value: session_control_issue_value(issue).to_owned(),
            });
        }
    }
}

fn session_control_issue_value(issue: &SessionControlIssue) -> &str {
    match *issue {
        SessionControlIssue::InvalidValue { ref value, .. } => value,
        SessionControlIssue::EmptyValue
        | SessionControlIssue::MultipleValues { .. }
        | SessionControlIssue::InvalidJson
        | SessionControlIssue::NotObject => "",
    }
}

/// Inspects a fixed-format v1 durable session control file body.
#[must_use]
pub fn inspect_session_control(kind: SessionControlKind, content: &str) -> SessionControlReport {
    match kind {
        SessionControlKind::State => inspect_session_state_control(content),
        SessionControlKind::Cwd => inspect_session_cwd_control(content),
        SessionControlKind::MetaJson => inspect_session_meta_json(content),
    }
}

fn inspect_session_state_control(content: &str) -> SessionControlReport {
    inspect_single_session_control_value(content, |line, value, issues| {
        if !matches!(value, "active" | "idle" | "done" | "error" | "cancelled") {
            issues.push(SessionControlIssue::InvalidValue {
                line,
                value: value.to_owned(),
            });
        }
    })
}

fn inspect_session_cwd_control(content: &str) -> SessionControlReport {
    inspect_single_session_control_value(content, |line, value, issues| {
        if !is_stable_chroot_absolute_path(value) {
            issues.push(SessionControlIssue::InvalidValue {
                line,
                value: value.to_owned(),
            });
        }
    })
}

fn inspect_single_session_control_value(
    content: &str,
    validate: impl Fn(usize, &str, &mut Vec<SessionControlIssue>),
) -> SessionControlReport {
    let mut issues = Vec::new();
    let lines = content.lines().collect::<Vec<_>>();
    let value = lines.first().map_or("", |line| line.trim());
    if value.is_empty() {
        issues.push(SessionControlIssue::EmptyValue);
    } else if lines.first().is_some_and(|line| *line != value) {
        issues.push(SessionControlIssue::InvalidValue {
            line: 1,
            value: value.to_owned(),
        });
    } else {
        validate(1, value, &mut issues);
    }
    if lines.len() > 1 {
        issues.push(SessionControlIssue::MultipleValues { line: 2 });
    }
    SessionControlReport::new(issues)
}

fn inspect_session_meta_json(content: &str) -> SessionControlReport {
    if !content.trim_start().starts_with('{') {
        if serde_json::from_str::<Value>(content).is_ok() {
            return SessionControlReport::new(vec![SessionControlIssue::NotObject]);
        }
        return SessionControlReport::new(vec![SessionControlIssue::InvalidJson]);
    }
    let Ok(meta) = serde_path_to_error::deserialize::<_, SessionMetaJson>(
        &mut serde_json::Deserializer::from_str(content),
    ) else {
        return SessionControlReport::new(vec![SessionControlIssue::InvalidJson]);
    };

    let mut issues = Vec::new();
    inspect_optional_meta_string(meta.client.as_ref(), "client", &mut issues, |_| true);
    inspect_optional_meta_string(meta.model.as_ref(), "model", &mut issues, is_model_name);
    inspect_optional_meta_string(meta.scope.as_ref(), "scope", &mut issues, |scope| {
        matches!(scope, "private" | "shared" | "temp")
    });
    SessionControlReport::new(issues)
}

#[derive(Deserialize)]
struct SessionMetaJson {
    client: Option<JsonStringField>,
    model: Option<JsonStringField>,
    scope: Option<JsonStringField>,
}

fn inspect_optional_meta_string(
    value: Option<&JsonStringField>,
    field: &str,
    issues: &mut Vec<SessionControlIssue>,
    valid: impl Fn(&str) -> bool,
) {
    match value {
        None => {}
        Some(value) => match *value {
            JsonStringField::String(ref text) if !valid(text) => {
                issues.push(SessionControlIssue::InvalidValue {
                    line: 1,
                    value: text.clone(),
                });
            }
            JsonStringField::String(_) => {}
            JsonStringField::Other(ref value) => {
                let _ = value;
                issues.push(SessionControlIssue::InvalidValue {
                    line: 1,
                    value: field.to_owned(),
                });
            }
        },
    }
}

pub(crate) fn is_stable_chroot_absolute_path(value: &str) -> bool {
    if !value.starts_with('/')
        || value.contains('\0')
        || value.contains('\t')
        || value.contains('\n')
    {
        return false;
    }
    if value == "/" {
        return true;
    }
    value
        .split('/')
        .skip(1)
        .all(|part| !part.is_empty() && part != "." && part != "..")
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonStringField {
    String(String),
    Other(Value),
}

impl JsonStringField {
    fn as_str(&self) -> Option<&str> {
        match *self {
            Self::String(ref value) => Some(value),
            Self::Other(ref value) => {
                let _ = value;
                None
            }
        }
    }
}

fn inspect_child_result_dirs(child_root: &Path, issues: &mut Vec<SessionLayoutIssue>) {
    let Ok(entries) = fs::read_dir(child_root) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let child_name = entry.file_name().to_string_lossy().into_owned();
        inspect_child_result_dir(&path, &child_name, issues);
    }
}

fn inspect_child_result_dir(
    child_dir: &Path,
    child_name: &str,
    issues: &mut Vec<SessionLayoutIssue>,
) {
    for file in CHILD_RESULT_REQUIRED_FILES {
        let label = format!("context/child/{child_name}/{file}");
        require_file(&child_dir.join(file), &label, issues);
    }
    for dir in CHILD_RESULT_REQUIRED_DIRS {
        let label = format!("context/child/{child_name}/{dir}");
        require_directory(&child_dir.join(dir), &label, issues);
    }
}

fn require_file(path: &Path, label: &str, issues: &mut Vec<SessionLayoutIssue>) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_metadata) => issues.push(SessionLayoutIssue::NotFile(label.to_owned())),
        Err(_error) => issues.push(SessionLayoutIssue::MissingFile(label.to_owned())),
    }
}

fn require_directory(path: &Path, label: &str, issues: &mut Vec<SessionLayoutIssue>) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_metadata) => issues.push(SessionLayoutIssue::NotDirectory(label.to_owned())),
        Err(_error) => issues.push(SessionLayoutIssue::MissingDirectory(label.to_owned())),
    }
}

/// Decides whether an agent may execute a tool through `CTX_PATH`.
///
/// This is a pure effective-authority check for the stable tool boundary:
/// the selected tool must be executable for the agent's Linux identity, visible
/// through a mount that is not `noexec`, and allowed by both the agent policy
/// and the tool object's own policy. Tool schemas, prompts, skills, and MCP
/// config files are intentionally not inputs because they never grant
/// authority. Model principals are refused before policy is considered.
pub fn authorize_tool_execution(
    tool_path: &ToolPath,
    tool_name: &str,
    authority: ToolExecutionAuthority<'_>,
) -> Result<ToolExecutionGrant, ToolExecutionDenial> {
    if !is_object_name(tool_name) {
        return Err(ToolExecutionDenial::InvalidToolName);
    }
    if authority.principal == ToolExecutionPrincipal::Model {
        return Err(ToolExecutionDenial::ModelCannotExecute);
    }
    let hit = tool_path
        .find(tool_name)
        .map_err(tool_path_denial)?
        .ok_or(ToolExecutionDenial::ToolNotFound)?;

    let metadata =
        fs::metadata(hit.path()).map_err(|_error| ToolExecutionDenial::CannotInspectTool)?;
    if !linux_identity_can_execute(&metadata, authority.identity) {
        return Err(ToolExecutionDenial::LinuxPermission);
    }

    let mount = most_specific_mount_for_path(authority.mount_table, hit.path())
        .ok_or(ToolExecutionDenial::NotMounted)?;
    if mount.options().contains(&MountOption::NoExec) {
        return Err(ToolExecutionDenial::NoExecMount);
    }

    if !authority.agent_policy.allows(
        authority.agent_subject,
        PolicyObjectClass::Tool,
        tool_name,
        PolicyPermission::Execute,
    ) {
        return Err(ToolExecutionDenial::AgentPolicy);
    }

    if !authority.tool_policy.allows(
        authority.agent_subject,
        PolicyObjectClass::Tool,
        tool_name,
        PolicyPermission::Execute,
    ) {
        return Err(ToolExecutionDenial::ToolPolicy);
    }

    Ok(ToolExecutionGrant::new(hit))
}

/// Decides whether an agent may access a stable shared-space path.
///
/// Shared access is default-deny and requires all of: a stable shared path for
/// the named space, mount visibility, read-write mount mode for writes, Linux
/// uid/gid/groups/mode permission, and policy v0 permission.
pub fn authorize_shared_access(
    shared_name: &str,
    path: &Path,
    access: SharedAccess,
    authority: SharedAccessAuthority<'_>,
) -> Result<(), SharedAccessDenial> {
    if !is_object_name(shared_name) {
        return Err(SharedAccessDenial::InvalidSharedName);
    }

    let mount = most_specific_mount_for_path(authority.mount_table, path)
        .ok_or(SharedAccessDenial::NotMounted)?;
    if !is_stable_shared_mount_for(mount, shared_name) {
        return Err(SharedAccessDenial::WrongSharedPath);
    }
    if access == SharedAccess::Write && mount.mode() == MountMode::ReadOnly {
        return Err(SharedAccessDenial::ReadOnlyMount);
    }

    let metadata = fs::metadata(path).map_err(|_error| SharedAccessDenial::CannotInspectPath)?;
    let linux_allowed = match access {
        SharedAccess::Read => linux_identity_can_read(&metadata, authority.identity),
        SharedAccess::Write => linux_identity_can_write(&metadata, authority.identity),
    };
    if !linux_allowed {
        return Err(SharedAccessDenial::LinuxPermission);
    }

    if !authority.policy.allows(
        authority.agent_subject,
        PolicyObjectClass::Shared,
        shared_name,
        access.policy_permission(),
    ) {
        return Err(SharedAccessDenial::Policy);
    }

    Ok(())
}

/// Decides whether an agent may access a durable private or shared session path.
///
/// Session access is default-deny and requires mount visibility, mount write
/// mode for writes, Linux uid/gid/groups/mode permission, and policy v0
/// `session:<name>` permission. Shared sessions additionally require matching
/// `shared:<space>` policy, so one IM channel cannot read another channel's
/// memory just because both are under `shared/`.
pub fn authorize_session_access(
    path: &Path,
    access: SessionAccess,
    authority: SessionAccessAuthority<'_>,
) -> Result<(), SessionAccessDenial> {
    let mount = most_specific_mount_for_path(authority.mount_table, path)
        .ok_or(SessionAccessDenial::NotMounted)?;
    let session =
        mounted_session_path(mount, path).ok_or(SessionAccessDenial::InvalidSessionPath)?;
    if access == SessionAccess::Write && mount.mode() == MountMode::ReadOnly {
        return Err(SessionAccessDenial::ReadOnlyMount);
    }

    let metadata = fs::metadata(path).map_err(|_error| SessionAccessDenial::CannotInspectPath)?;
    let linux_allowed = match access {
        SessionAccess::Read | SessionAccess::Resume => {
            linux_identity_can_read(&metadata, authority.identity)
        }
        SessionAccess::Write => linux_identity_can_write(&metadata, authority.identity),
    };
    if !linux_allowed || !session.home_uid_allows(authority.identity) {
        return Err(SessionAccessDenial::LinuxPermission);
    }

    if let Some(shared_name) = session.shared_name()
        && !authority.policy.allows(
            authority.agent_subject,
            PolicyObjectClass::Shared,
            shared_name,
            access.shared_policy_permission(),
        )
    {
        return Err(SessionAccessDenial::SharedPolicy);
    }

    if !authority.policy.allows(
        authority.agent_subject,
        PolicyObjectClass::Session,
        session.session_name(),
        access.policy_permission(),
    ) {
        return Err(SessionAccessDenial::SessionPolicy);
    }

    Ok(())
}

/// Decides whether a requested child agent is attenuated from its parent.
///
/// v1 supports only owned children. This check keeps child creation in the
/// ordinary agent object/control-file ABI while proving that the child cannot
/// expand identity, groups, policy, or mount visibility.
pub fn authorize_child_agent(
    request: ChildAgentRequest<'_>,
    authority: ChildAgentAuthority<'_>,
) -> Result<(), ChildAgentDenial> {
    if !is_object_name(request.child_name) {
        return Err(ChildAgentDenial::InvalidChildName);
    }
    if !is_object_name(authority.parent_agent) {
        return Err(ChildAgentDenial::InvalidParentName);
    }
    if !is_object_name(request.controls.subject) || !is_object_name(authority.subject) {
        return Err(ChildAgentDenial::InvalidSubject);
    }
    if !parent_ref_matches(request.parent_ref, authority.parent_agent)? {
        return Err(ChildAgentDenial::ParentMismatch);
    }
    if request.lifecycle != ChildLifecycle::Owned {
        return Err(ChildAgentDenial::UnsupportedLifecycle);
    }
    if request.controls.identity.uid() != authority.identity.uid()
        || request.controls.identity.gid() != authority.identity.gid()
    {
        return Err(ChildAgentDenial::IdentityExpansion);
    }
    if !groups_are_subset(
        request.controls.identity.groups(),
        authority.identity.groups(),
    ) {
        return Err(ChildAgentDenial::GroupExpansion);
    }
    if !request.controls.policy.is_authority_subset_of(
        authority.effective_policy,
        request.controls.subject,
        authority.subject,
    ) {
        return Err(ChildAgentDenial::PolicyExpansion);
    }
    if !request
        .controls
        .mounts
        .is_subset_of(authority.visible_mounts)
    {
        return Err(ChildAgentDenial::MountExpansion);
    }

    Ok(())
}

/// Builds the canonical event pair for owned child cancellation caused by
/// parent death.
pub fn owned_child_cancellation_events(
    parent_agent: &str,
    child_agent: &str,
) -> Result<OwnedChildCancellationEvents, OwnedChildCancellationError> {
    if !is_object_name(parent_agent) {
        return Err(OwnedChildCancellationError::InvalidParentName);
    }
    if !is_object_name(child_agent) {
        return Err(OwnedChildCancellationError::InvalidChildName);
    }

    Ok(OwnedChildCancellationEvents {
        parent_event: serde_json::json!({
            "type": "agent.child.cancel",
            "parent": parent_agent,
            "child": child_agent,
            "reason": "parent_dead"
        })
        .to_string(),
        child_event: serde_json::json!({
            "type": "agent.stop",
            "agent": child_agent,
            "status": "cancelled"
        })
        .to_string(),
    })
}

/// Records the durable filesystem effects of cancelling an owned child runtime.
///
/// This function does not supervise or signal processes. It is the auditable
/// state transition a runtime calls after parent death cancellation: child
/// history remains in place, the child session state becomes `cancelled`, and
/// canonical lifecycle events are appended to the existing session logs.
pub fn record_owned_child_cancellation(
    parent_agent: &str,
    child_agent: &str,
    parent_session_dir: &Path,
    child_session_dir: &Path,
) -> Result<OwnedChildCancellationEvents, OwnedChildCancellationError> {
    let events = owned_child_cancellation_events(parent_agent, child_agent)?;
    let parent_events = parent_session_dir.join("events.jsonl");
    let child_messages = child_session_dir.join("messages.jsonl");
    let child_events = child_session_dir.join("events.jsonl");
    let child_state = child_session_dir.join("state");

    if !parent_events.is_file() {
        return Err(OwnedChildCancellationError::MissingParentEvents);
    }
    if !child_messages.is_file() || !child_events.is_file() || !child_state.is_file() {
        return Err(OwnedChildCancellationError::MissingChildHistory);
    }

    atomic_replace_text(&child_state, "cancelled\n")
        .map_err(|_error| OwnedChildCancellationError::CannotRecord)?;
    append_jsonl_event(&parent_events, events.parent_event())
        .map_err(|_error| OwnedChildCancellationError::CannotRecord)?;
    append_jsonl_event(&child_events, events.child_event())
        .map_err(|_error| OwnedChildCancellationError::CannotRecord)?;

    Ok(events)
}

fn parent_ref_matches(value: &str, parent_agent: &str) -> Result<bool, ChildAgentDenial> {
    Ok(parent_ref_agent_name(value)? == parent_agent)
}

pub(crate) fn parent_ref_agent_name(value: &str) -> Result<&str, ChildAgentDenial> {
    let mut fields = value.split_whitespace();
    let Some(agent) = fields.next() else {
        return Err(ChildAgentDenial::InvalidParentRef);
    };
    let Some(agent_name) = agent.strip_prefix("agent:") else {
        return Err(ChildAgentDenial::InvalidParentRef);
    };
    if !is_object_name(agent_name) {
        return Err(ChildAgentDenial::InvalidParentRef);
    }

    for field in fields {
        let Some((kind, value)) = field.split_once(':') else {
            return Err(ChildAgentDenial::InvalidParentRef);
        };
        if !matches!(kind, "session" | "run") || !is_object_name(value) {
            return Err(ChildAgentDenial::InvalidParentRef);
        }
    }

    Ok(agent_name)
}

fn groups_are_subset(child_groups: &[u32], parent_groups: &[u32]) -> bool {
    child_groups
        .iter()
        .all(|child_group| parent_groups.contains(child_group))
}

fn append_jsonl_event(path: &Path, event: &str) -> std::io::Result<()> {
    append_jsonl_line(path, event)
}

fn append_jsonl_line(path: &Path, line: &str) -> std::io::Result<()> {
    let mut file = fs::OpenOptions::new().append(true).open(path)?;
    file.write_all(line.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()
}

pub(crate) fn atomic_replace_text(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(content.as_bytes())?;
    temp.flush()?;
    temp.as_file()
        .set_permissions(fs::Permissions::from_mode(0o644))?;
    temp.persist(path)
        .map(|_file| ())
        .map_err(std::io::Error::from)
}

fn unix_timestamp_text() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    format!("{seconds}\n")
}

fn tool_path_denial(error: ToolPathError) -> ToolExecutionDenial {
    match error {
        ToolPathError::InvalidName => ToolExecutionDenial::InvalidToolName,
        ToolPathError::CannotReadDirectory => ToolExecutionDenial::CannotReadToolPath,
    }
}

fn linux_identity_can_execute(metadata: &fs::Metadata, identity: &AgentUnixIdentity) -> bool {
    let mode = metadata.permissions().mode();
    if identity.uid() == 0 {
        return mode & 0o111 != 0;
    }
    if metadata.uid() == identity.uid() {
        return mode & 0o100 != 0;
    }
    if identity.is_in_group(metadata.gid()) {
        return mode & 0o010 != 0;
    }
    mode & 0o001 != 0
}

fn linux_identity_can_read(metadata: &fs::Metadata, identity: &AgentUnixIdentity) -> bool {
    linux_identity_has_mode(metadata, identity, 0o400, 0o040, 0o004, 0o444)
}

fn linux_identity_can_write(metadata: &fs::Metadata, identity: &AgentUnixIdentity) -> bool {
    linux_identity_has_mode(metadata, identity, 0o200, 0o020, 0o002, 0o222)
}

fn linux_identity_has_mode(
    metadata: &fs::Metadata,
    identity: &AgentUnixIdentity,
    owner_bit: u32,
    group_bit: u32,
    other_bit: u32,
    root_mask: u32,
) -> bool {
    let mode = metadata.permissions().mode();
    if identity.uid() == 0 {
        return mode & root_mask != 0;
    }
    if metadata.uid() == identity.uid() {
        return mode & owner_bit != 0;
    }
    if identity.is_in_group(metadata.gid()) {
        return mode & group_bit != 0;
    }
    mode & other_bit != 0
}

fn most_specific_mount_for_path<'a>(
    mount_table: &'a MountTable,
    path: &Path,
) -> Option<&'a MountEntry> {
    mount_table
        .entries()
        .iter()
        .filter(|entry| path.starts_with(Path::new(entry.target())))
        .max_by_key(|entry| Path::new(entry.target()).components().count())
}

fn is_stable_shared_mount_for(mount: &MountEntry, shared_name: &str) -> bool {
    is_stable_shared_path_for(Path::new(mount.source()), shared_name)
        || is_stable_shared_path_for(Path::new(mount.target()), shared_name)
}

fn is_stable_shared_path_for(path: &Path, shared_name: &str) -> bool {
    let mut parts = path.components().filter_map(|component| match component {
        std::path::Component::Normal(value) => value.to_str(),
        std::path::Component::RootDir
        | std::path::Component::CurDir
        | std::path::Component::ParentDir
        | std::path::Component::Prefix(_) => None,
    });

    match (parts.next(), parts.next(), parts.next()) {
        (Some("ctx"), Some("shared"), Some(name)) | (Some("shared"), Some(name), _) => {
            name == shared_name
        }
        _ => false,
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum MountedSessionPath {
    Private {
        uid: u32,
        agent: String,
        session: String,
    },
    Shared {
        shared: String,
        agent: String,
        session: String,
    },
}

impl MountedSessionPath {
    fn session_name(&self) -> &str {
        match *self {
            Self::Private { ref session, .. } | Self::Shared { ref session, .. } => session,
        }
    }

    fn shared_name(&self) -> Option<&str> {
        match *self {
            Self::Shared { ref shared, .. } => Some(shared),
            Self::Private { .. } => None,
        }
    }

    fn home_uid_allows(&self, identity: &AgentUnixIdentity) -> bool {
        match *self {
            Self::Private { uid, .. } => uid == identity.uid(),
            Self::Shared { .. } => true,
        }
    }
}

fn mounted_session_path(mount: &MountEntry, path: &Path) -> Option<MountedSessionPath> {
    let stable = mounted_stable_path(mount, path)?;
    parse_mounted_session_path(&stable)
}

fn mounted_stable_path(mount: &MountEntry, path: &Path) -> Option<PathBuf> {
    let target = Path::new(mount.target());
    if let Ok(relative) = path.strip_prefix(target) {
        return Some(Path::new(mount.source()).join(relative));
    }
    path.starts_with(Path::new(mount.source()))
        .then(|| path.to_path_buf())
}

fn parse_mounted_session_path(path: &Path) -> Option<MountedSessionPath> {
    let parts = stable_path_parts(path)?;
    match *parts.as_slice() {
        ["ctx", "home", uid, "agent", agent, "session", session, ..]
        | ["home", uid, "agent", agent, "session", session, ..] => {
            let uid = uid.parse::<u32>().ok()?;
            (is_object_name(agent) && is_object_name(session)).then(|| {
                MountedSessionPath::Private {
                    uid,
                    agent: (*agent).to_owned(),
                    session: (*session).to_owned(),
                }
            })
        }
        [
            "ctx",
            "shared",
            shared,
            "agent",
            agent,
            "session",
            session,
            ..,
        ]
        | ["shared", shared, "agent", agent, "session", session, ..] => (is_object_name(shared)
            && is_object_name(agent)
            && is_object_name(session))
        .then(|| MountedSessionPath::Shared {
            shared: (*shared).to_owned(),
            agent: (*agent).to_owned(),
            session: (*session).to_owned(),
        }),
        _ => None,
    }
}

fn stable_path_parts(path: &Path) -> Option<Vec<&str>> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(value) => parts.push(value.to_str()?),
            std::path::Component::RootDir => {}
            std::path::Component::CurDir
            | std::path::Component::ParentDir
            | std::path::Component::Prefix(_) => return None,
        }
    }
    Some(parts)
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/lib_tests.rs"
    ));
}
