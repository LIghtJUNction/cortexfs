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
mod abi_path_parse;
mod agent_control;
mod context_jsonl;
mod context_pack;
mod context_pack_build;
mod context_pack_inspect;
mod context_pack_source;
mod message_stream;
mod model;
mod mount_table;
mod policy;
mod session_index;
mod session_layout;
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
pub use session_layout::{
    SessionControlIssue, SessionControlKind, SessionControlReport, SessionLayoutIssue,
    SessionLayoutReport, inspect_session_control, inspect_session_layout,
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

include!("fuse_v1_types.rs");

/// Error while reading Unix socket peer credentials.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeerCredentialError {
    /// Kernel peer credential lookup failed.
    CannotRead,
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

include!("fuse_v1_projection.rs");

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

include!("fuse_v1_provider.rs");

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

include!("socket_runtime.rs");

include!("socket_session_record.rs");

include!("agent_runtime_view.rs");

include!("object_metadata.rs");

include!("object_bootstrap.rs");

include!("reference_tree_bootstrap.rs");

include!("reference_tree_helpers.rs");

include!("fuse_v1_path.rs");

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

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonU64Field {
    Number(u64),
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

fn is_json_u64(value: Option<&JsonU64Field>) -> bool {
    value.is_some_and(|value| match *value {
        JsonU64Field::Number(ref number) => {
            let _ = number;
            true
        }
        JsonU64Field::Other(ref value) => {
            let _ = value;
            false
        }
    })
}

fn provider_native_fields(value: &Value) -> Vec<&str> {
    let mut fields = Vec::new();
    collect_provider_native_fields(value, &mut fields);
    fields
}

fn collect_provider_native_fields<'a>(value: &'a Value, fields: &mut Vec<&'a str>) {
    if let Some(object) = value.as_object() {
        for (key, child) in object {
            if is_provider_native_field(key) {
                fields.push(key);
            }
            collect_provider_native_fields(child, fields);
        }
        return;
    }

    if let Some(items) = value.as_array() {
        for item in items {
            collect_provider_native_fields(item, fields);
        }
    }
}

fn is_provider_native_field(key: &str) -> bool {
    matches!(
        key,
        "thread_id"
            | "response_id"
            | "conversation_id"
            | "provider_thread_id"
            | "provider_response_id"
            | "native_thread"
            | "native_state"
            | "openai_response_id"
            | "anthropic_message_id"
            | "gemini_response_id"
    )
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
