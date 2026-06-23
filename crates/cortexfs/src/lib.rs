//! `CortexFS` Agent OS ABI design core.
//!
//! The old CLI, daemon, provider registry, and FUSE projection were removed
//! before the Agent OS rewrite. This crate intentionally exposes only stable
//! ABI names while the implementation is redesigned around Rig.

use std::fmt::Write as FmtWrite;
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt, symlink};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use nix::sys::socket::{getsockopt, sockopt};
use serde_json::Value;

/// Default `CortexFS` mount root.
pub const CTX_ROOT: &str = "/ctx";

/// Root entries reserved by the new Agent OS ABI.
pub const ROOT_ENTRIES: &[&str] = &["status", "bin", "model", "agent", "tool", "home", "shared"];

/// Object classes exposed as executable files.
pub const EXEC_OBJECTS: &[&str] = &["model", "agent", "tool"];

/// Maximum object name length.
pub const MAX_OBJECT_NAME_LEN: usize = 64;

/// Required model control files.
pub const MODEL_CONTROL_FILES: &[&str] =
    &["id", "driver", "cap", "default", "session", "status", "log"];

/// Stable semantic model capability words in the v1 ABI.
pub const STABLE_MODEL_CAPABILITIES: &[&str] = &[
    "chat",
    "stream",
    "session",
    "vision",
    "audio_input",
    "audio_output",
    "json_schema",
    "tool_call_syntax",
    "reasoning",
    "embedding",
    "rerank",
];

/// Provider/API-format-private capability words forbidden in the v1 ABI.
pub const FORBIDDEN_MODEL_CAPABILITIES: &[&str] = &[
    "openai_responses",
    "anthropic_messages",
    "gemini_generate_content",
    "native_thread",
    "native_stateful",
    "native_stateless",
];

/// Required agent control files.
pub const AGENT_CONTROL_FILES: &[&str] = &[
    "owner",
    "uid",
    "gid",
    "groups",
    "label",
    "iso",
    "parent",
    "life",
    "root",
    "cwd",
    "env",
    "path",
    "mount",
    "model",
    "policy",
    "status",
    "pid",
    "log",
    "meta.json",
];

/// Required tool control files.
pub const TOOL_CONTROL_FILES: &[&str] = &[
    "name",
    "description",
    "schema",
    "cap",
    "policy",
    "status",
    "log",
];

/// Required durable files in a v1 agent session directory.
pub const SESSION_REQUIRED_FILES: &[&str] = &[
    "messages.jsonl",
    "events.jsonl",
    "latest.md",
    "state",
    "cwd",
    "created_at",
    "updated_at",
    "meta.json",
];

/// Required derived/rebuildable context files for transparency.
pub const CONTEXT_REQUIRED_FILES: &[&str] = &[
    "budget",
    "pack.json",
    "pack.md",
    "summary.md",
    "facts.jsonl",
    "decisions.jsonl",
    "todo.md",
    "refs.jsonl",
];

/// Required context subdirectories.
pub const CONTEXT_REQUIRED_DIRS: &[&str] = &["pinned", "swap", "dedup", "child"];

/// Required files in each parent-owned child result directory.
pub const CHILD_RESULT_REQUIRED_FILES: &[&str] = &[
    "agent",
    "session",
    "status",
    "handoff.md",
    "result.md",
    "refs.jsonl",
];

/// Required directories in each parent-owned child result directory.
pub const CHILD_RESULT_REQUIRED_DIRS: &[&str] = &["artifact"];

/// Required directories in a shared project queue.
pub const SHARED_QUEUE_REQUIRED_DIRS: &[&str] =
    &["inbox", "pending", "lease", "claimed", "done", "failed"];

/// Maximum v1 JSONL socket request frame size.
pub const MAX_SOCKET_FRAME_BYTES: usize = 1024 * 1024;

/// Error while resolving tool lookup through `CTX_PATH`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToolPathError {
    /// Tool name is not a valid v1 object name.
    InvalidName,
    /// Reading a lookup directory failed for a reason other than it not existing.
    CannotReadDirectory,
}

/// Maximum payload accepted by the v1 local FUSE projection for one small write.
pub const MAX_FUSE_V1_SMALL_WRITE_BYTES: usize = 64 * 1024;

/// Stable inode id for the v1 `/ctx` root in a FUSE adapter.
pub const FUSE_V1_ROOT_INODE: u64 = 1;

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
}

/// Policy syntax error for the fixed v0 allowlist.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyError {
    /// Rule must use the `allow` keyword.
    ExpectedAllow,
    /// Rule must have exactly four fields.
    WrongFieldCount,
    /// Object must use `class:name` form.
    InvalidObject,
    /// Subject type or object name is invalid.
    InvalidName,
    /// Object class is not in the fixed v1 set.
    UnknownClass,
    /// Permission is not valid for the object class.
    UnknownPermission,
}

/// Mount file syntax error for the fixed v0 mount table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MountError {
    /// Mount line must have exactly four tab-separated fields.
    WrongFieldCount,
    /// Source and target must be absolute paths without tab or newline.
    InvalidPath,
    /// Mode must be `ro` or `rw`.
    InvalidMode,
    /// Option set must be one of the fixed v0 words.
    InvalidOption,
    /// Options other than `-` must not repeat.
    DuplicateOption,
    /// `bind` and `rbind` are mutually exclusive.
    ConflictingBindOption,
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

/// Context pack validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextPackIssue {
    /// `pack.json` is not valid JSON.
    InvalidJson,
    /// Root JSON value does not contain an `items` array.
    ItemsNotArray,
    /// Pack item is not a JSON object.
    ItemNotObject(usize),
    /// Pack item does not identify an inspectable source.
    MissingSource(usize),
    /// Pack item `source` is present but is not a string.
    SourceNotString(usize),
    /// Pack item source is outside the allowed session-relative source set.
    InvalidSource {
        /// Zero-based item index.
        item: usize,
        /// Source value from the pack.
        source: String,
        /// Stable reason for refusal.
        reason: ContextPackSourceError,
    },
}

impl ContextPackIssue {
    /// Returns a stable short description of the issue kind.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match *self {
            Self::InvalidJson => "invalid json",
            Self::ItemsNotArray => "items not array",
            Self::ItemNotObject(_) => "item not object",
            Self::MissingSource(_) => "missing source",
            Self::SourceNotString(_) => "source not string",
            Self::InvalidSource { .. } => "invalid source",
        }
    }

    /// Returns the pack item index associated with this issue, when any.
    #[must_use]
    pub const fn item(&self) -> Option<usize> {
        match *self {
            Self::ItemNotObject(index)
            | Self::MissingSource(index)
            | Self::SourceNotString(index)
            | Self::InvalidSource { item: index, .. } => Some(index),
            Self::InvalidJson | Self::ItemsNotArray => None,
        }
    }

    /// Returns the rejected source string, when any.
    #[must_use]
    pub fn source(&self) -> Option<&str> {
        match *self {
            Self::InvalidSource { ref source, .. } => Some(source),
            Self::InvalidJson
            | Self::ItemsNotArray
            | Self::ItemNotObject(_)
            | Self::MissingSource(_)
            | Self::SourceNotString(_) => None,
        }
    }

    /// Returns the source rejection reason, when any.
    #[must_use]
    pub const fn source_reason(&self) -> Option<ContextPackSourceError> {
        match *self {
            Self::InvalidSource { reason, .. } => Some(reason),
            Self::InvalidJson
            | Self::ItemsNotArray
            | Self::ItemNotObject(_)
            | Self::MissingSource(_)
            | Self::SourceNotString(_) => None,
        }
    }
}

/// Stable reason a context pack source is refused.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextPackSourceError {
    /// Source is empty.
    Empty,
    /// Source is absolute instead of relative to the owning session.
    Absolute,
    /// Source contains an empty path component.
    EmptyComponent,
    /// Source contains `.`.
    DotComponent,
    /// Source contains `..`.
    ParentComponent,
    /// Source names a child result path outside the allowed parent-owned result channel.
    UnsupportedChildPath,
    /// Source is neither a durable session file nor a `context/` path.
    UnsupportedSessionPath,
}

impl ContextPackSourceError {
    /// Returns a stable short reason string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Absolute => "absolute",
            Self::EmptyComponent => "empty component",
            Self::DotComponent => "dot component",
            Self::ParentComponent => "parent component",
            Self::UnsupportedChildPath => "unsupported child path",
            Self::UnsupportedSessionPath => "unsupported session path",
        }
    }
}

/// Result of inspecting `context/pack.json` source transparency.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContextPackReport {
    issues: Vec<ContextPackIssue>,
}

/// One source selected into a rebuilt context pack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContextPackBuiltItem {
    kind: String,
    source: String,
    range: Option<String>,
    tokens: u64,
}

/// Result of rebuilding `context/pack.json` and `context/pack.md`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContextPackBuild {
    items: Vec<ContextPackBuiltItem>,
    pack_json: String,
    pack_md: String,
}

/// Error while rebuilding an inspectable context pack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextPackBuildError {
    /// Session directory name is not a valid v1 session name.
    InvalidSessionName,
    /// Optional agent name is not a valid v1 object name.
    InvalidAgentName,
    /// Required durable session files or context directory are missing.
    MissingSession,
    /// `context/budget` is not empty or a single unsigned integer value.
    InvalidBudget,
    /// `messages.jsonl` is not valid canonical durable message history.
    InvalidMessages,
    /// A context JSONL source selected for the pack is invalid.
    InvalidContextJsonl,
    /// A child result directory name is not a valid v1 object name.
    InvalidChildName,
    /// Session/context files could not be read.
    CannotRead,
    /// `context/pack.json` or `context/pack.md` could not be written.
    CannotRecord,
}

impl ContextPackReport {
    /// Creates a report with collected context pack issues.
    #[must_use]
    pub const fn new(issues: Vec<ContextPackIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when all pack items identify allowed session-relative sources.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected pack issues.
    #[must_use]
    pub fn issues(&self) -> &[ContextPackIssue] {
        &self.issues
    }
}

impl ContextPackBuiltItem {
    /// Creates a selected pack item.
    #[must_use]
    pub fn new(kind: &str, source: &str, range: Option<String>, tokens: u64) -> Self {
        Self {
            kind: kind.to_owned(),
            source: source.to_owned(),
            range,
            tokens,
        }
    }

    /// Returns the pack item kind.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// Returns the session-relative source.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the optional item range.
    #[must_use]
    pub fn range(&self) -> Option<&str> {
        self.range.as_deref()
    }

    /// Returns the approximate token count used for pack budgeting.
    #[must_use]
    pub const fn tokens(&self) -> u64 {
        self.tokens
    }
}

impl ContextPackBuild {
    /// Creates a context pack build result.
    #[must_use]
    pub const fn new(items: Vec<ContextPackBuiltItem>, pack_json: String, pack_md: String) -> Self {
        Self {
            items,
            pack_json,
            pack_md,
        }
    }

    /// Returns the selected pack items.
    #[must_use]
    pub fn items(&self) -> &[ContextPackBuiltItem] {
        &self.items
    }

    /// Returns the generated `context/pack.json` body.
    #[must_use]
    pub fn pack_json(&self) -> &str {
        &self.pack_json
    }

    /// Returns the generated `context/pack.md` body.
    #[must_use]
    pub fn pack_md(&self) -> &str {
        &self.pack_md
    }
}

impl ContextPackBuildError {
    /// Returns a stable errno name for this context pack rebuild failure.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidSessionName
            | Self::InvalidAgentName
            | Self::InvalidBudget
            | Self::InvalidMessages
            | Self::InvalidContextJsonl
            | Self::InvalidChildName => "EINVAL",
            Self::MissingSession => "ENOENT",
            Self::CannotRead | Self::CannotRecord => "EIO",
        }
    }
}

/// Canonical JSONL event stream validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EventStreamIssue {
    /// Line is not valid JSON.
    InvalidJson(usize),
    /// Event line is not a JSON object.
    EventNotObject(usize),
    /// Event does not have a `type` string.
    MissingType(usize),
    /// Event `type` is not in the stable v1 event set.
    UnknownType { line: usize, event_type: String },
    /// Event type requires a string `run` field.
    MissingRun(usize),
    /// Event contains a provider-native state field.
    ProviderNativeField { line: usize, field: String },
    /// Error event does not use a stable errno `code`.
    InvalidErrorCode(usize),
    /// Done event has an invalid `status`.
    InvalidDoneStatus(usize),
    /// Usage event lacks numeric token counts.
    InvalidUsage(usize),
    /// Tool call event lacks stable tool-call syntax.
    InvalidToolCall(usize),
    /// Agent lifecycle event lacks stable child-agent syntax.
    InvalidAgentLifecycle(usize),
}

/// Result of inspecting a canonical JSONL event stream.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct EventStreamReport {
    issues: Vec<EventStreamIssue>,
}

/// Canonical durable message history validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MessageStreamIssue {
    /// Line is not valid JSON.
    InvalidJson(usize),
    /// Message line is not a JSON object.
    MessageNotObject(usize),
    /// Message does not have a stable `role` string.
    MissingRole(usize),
    /// Message `role` is not in the stable v1 role set.
    InvalidRole { line: usize, role: String },
    /// Message does not have `content`.
    MissingContent(usize),
    /// Message `content` is neither a string nor a canonical content-part array.
    InvalidContent(usize),
    /// Message contains a provider-native state field.
    ProviderNativeField { line: usize, field: String },
}

/// Result of inspecting `messages.jsonl`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MessageStreamReport {
    issues: Vec<MessageStreamIssue>,
}

/// Stable context JSONL file kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ContextJsonlKind {
    /// `context/facts.jsonl`: stable fact records.
    Facts,
    /// `context/decisions.jsonl`: accepted decision records.
    Decisions,
    /// `context/refs.jsonl`: file, artifact, tool output, and swap refs.
    Refs,
    /// `context/swap/index.jsonl`: swapped-out prompt working-set refs.
    SwapIndex,
    /// `context/dedup/index.jsonl`: content-addressed dedup refs.
    DedupIndex,
}

/// Context JSONL validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextJsonlIssue {
    /// Line is not valid JSON.
    InvalidJson(usize),
    /// JSONL record is not a JSON object.
    RecordNotObject(usize),
    /// Required string field is missing or not a string.
    MissingStringField { line: usize, field: String },
    /// Required number field is missing or not an unsigned integer.
    MissingNumberField { line: usize, field: String },
    /// Required string-array field is missing or malformed.
    MissingStringArrayField { line: usize, field: String },
    /// Field value is outside the stable v1 syntax for this file.
    InvalidField {
        /// One-based JSONL line number.
        line: usize,
        /// Field name.
        field: String,
        /// Rejected value.
        value: String,
    },
}

/// Result of inspecting a context JSONL file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ContextJsonlReport {
    issues: Vec<ContextJsonlIssue>,
}

/// Tool schema control-file validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolSchemaIssue {
    /// `tool/<name>.d/schema` is not valid JSON.
    InvalidJson,
    /// Schema is valid JSON but not an object.
    NotObject,
    /// Top-level field tries to describe authority instead of input/output.
    AuthorityField(String),
}

/// Result of inspecting `tool/<name>.d/schema`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ToolSchemaReport {
    issues: Vec<ToolSchemaIssue>,
}

/// Model capability control-file validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelCapabilityIssue {
    /// Capability word is provider/API-format private.
    ProviderPrivate {
        /// One-based line number in `cap`.
        line: usize,
        /// Capability word from the file.
        capability: String,
    },
    /// Capability word is not in the stable v1 semantic capability set.
    Unknown {
        /// One-based line number in `cap`.
        line: usize,
        /// Capability word from the file.
        capability: String,
    },
}

/// Result of inspecting `model/<name>.d/cap`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelCapabilityReport {
    issues: Vec<ModelCapabilityIssue>,
}

/// Stable session index file kind.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionIndexKind {
    /// `session/index/list`: one session name per line.
    List,
    /// `session/index/current`: single current session name.
    Current,
    /// `session/index/by-cwd/<hash>`: single session name for a cwd hash.
    ByCwd,
}

/// Session index validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionIndexIssue {
    /// A required session name value is empty.
    EmptyValue { line: usize },
    /// A single-value index file contains more than one line.
    MultipleValues { line: usize },
    /// Session name does not use the stable object-name syntax.
    InvalidSessionName { line: usize, value: String },
}

/// Result of inspecting a session index file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SessionIndexReport {
    issues: Vec<SessionIndexIssue>,
}

/// Durable session index update error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionIndexUpdateError {
    /// Session name does not use the stable object-name syntax.
    InvalidSessionName,
    /// Optional `by-cwd` key does not use the stable object-name syntax.
    InvalidByCwdKey,
    /// The target durable session directory is missing.
    MissingSession,
    /// The reserved `session/index` directory or required files are missing.
    MissingIndex,
    /// Existing index files are malformed.
    InvalidIndex,
    /// Index files could not be read or atomically rewritten.
    CannotRecord,
}

impl SessionIndexUpdateError {
    /// Returns a stable errno name for this update failure.
    #[must_use]
    pub const fn errno(self) -> &'static str {
        match self {
            Self::InvalidSessionName | Self::InvalidByCwdKey | Self::InvalidIndex => "EINVAL",
            Self::MissingSession | Self::MissingIndex => "ENOENT",
            Self::CannotRecord => "EIO",
        }
    }
}

/// Stable agent control file kind with fixed v1 value syntax.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentControlKind {
    /// `agent/<name>.d/owner`: owning Linux uid.
    Owner,
    /// `agent/<name>.d/uid`: runtime Linux uid.
    Uid,
    /// `agent/<name>.d/gid`: runtime Linux gid.
    Gid,
    /// `agent/<name>.d/groups`: supplementary groups, one gid per line.
    Groups,
    /// `agent/<name>.d/iso`: isolation profile.
    Iso,
    /// `agent/<name>.d/parent`: parent agent/session/run reference.
    Parent,
    /// `agent/<name>.d/life`: lifecycle ownership.
    Life,
    /// `agent/<name>.d/status`: process lifecycle state.
    Status,
    /// `agent/<name>.d/pid`: runtime process id, when running.
    Pid,
}

impl AgentControlKind {
    /// Parses an agent control file name with fixed v1 syntax.
    #[must_use]
    pub fn parse(file_name: &str) -> Option<Self> {
        match file_name {
            "owner" => Some(Self::Owner),
            "uid" => Some(Self::Uid),
            "gid" => Some(Self::Gid),
            "groups" => Some(Self::Groups),
            "iso" => Some(Self::Iso),
            "parent" => Some(Self::Parent),
            "life" => Some(Self::Life),
            "status" => Some(Self::Status),
            "pid" => Some(Self::Pid),
            _ => None,
        }
    }
}

/// Agent control-file validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AgentControlIssue {
    /// A required single value is empty.
    EmptyValue,
    /// A single-value control file contains more than one line.
    MultipleValues { line: usize },
    /// Numeric uid/gid/pid value is malformed.
    InvalidNumber { line: usize, value: String },
    /// Fixed vocabulary or parent reference value is malformed.
    InvalidValue { line: usize, value: String },
}

/// Result of inspecting a fixed-format agent control file.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AgentControlReport {
    issues: Vec<AgentControlIssue>,
}

impl AgentControlReport {
    /// Creates a report with collected agent control issues.
    #[must_use]
    pub const fn new(issues: Vec<AgentControlIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when the control file satisfies the fixed v1 syntax.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected agent control issues.
    #[must_use]
    pub fn issues(&self) -> &[AgentControlIssue] {
        &self.issues
    }
}

impl SessionIndexReport {
    /// Creates a report with collected session index issues.
    #[must_use]
    pub const fn new(issues: Vec<SessionIndexIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when the index file satisfies the fixed v1 format.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected session index issues.
    #[must_use]
    pub fn issues(&self) -> &[SessionIndexIssue] {
        &self.issues
    }
}

impl ModelCapabilityReport {
    /// Creates a report with collected model capability issues.
    #[must_use]
    pub const fn new(issues: Vec<ModelCapabilityIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when all capabilities use stable v1 semantic words.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected capability issues.
    #[must_use]
    pub fn issues(&self) -> &[ModelCapabilityIssue] {
        &self.issues
    }
}

impl EventStreamReport {
    /// Creates a report with collected event stream issues.
    #[must_use]
    pub const fn new(issues: Vec<EventStreamIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when all events are stable v1 event frames.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected event stream issues.
    #[must_use]
    pub fn issues(&self) -> &[EventStreamIssue] {
        &self.issues
    }
}

impl MessageStreamReport {
    /// Creates a report with collected message stream issues.
    #[must_use]
    pub const fn new(issues: Vec<MessageStreamIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when all messages use stable v1 message frames.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected message stream issues.
    #[must_use]
    pub fn issues(&self) -> &[MessageStreamIssue] {
        &self.issues
    }
}

impl ContextJsonlReport {
    /// Creates a report with collected context JSONL issues.
    #[must_use]
    pub const fn new(issues: Vec<ContextJsonlIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when all records use the stable v1 context shape.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected context JSONL issues.
    #[must_use]
    pub fn issues(&self) -> &[ContextJsonlIssue] {
        &self.issues
    }
}

impl ToolSchemaReport {
    /// Creates a report with collected tool schema issues.
    #[must_use]
    pub const fn new(issues: Vec<ToolSchemaIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when the schema is a non-authority JSON object.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected tool schema issues.
    #[must_use]
    pub fn issues(&self) -> &[ToolSchemaIssue] {
        &self.issues
    }
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

/// Shared queue layout validation issue.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SharedQueueLayoutIssue {
    /// Required directory is missing.
    MissingDirectory(String),
    /// Path exists but is not a directory.
    NotDirectory(String),
}

/// Result of inspecting a shared queue directory.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SharedQueueLayoutReport {
    issues: Vec<SharedQueueLayoutIssue>,
}

impl SharedQueueLayoutReport {
    /// Creates a report with collected layout issues.
    #[must_use]
    pub const fn new(issues: Vec<SharedQueueLayoutIssue>) -> Self {
        Self { issues }
    }

    /// Returns true when the queue satisfies the v1 layout.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.issues.is_empty()
    }

    /// Returns all detected layout issues.
    #[must_use]
    pub fn issues(&self) -> &[SharedQueueLayoutIssue] {
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

/// Linux peer credentials for a connected Unix socket.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeerCredentials {
    pid: Option<i32>,
    uid: u32,
    gid: u32,
}

/// Stable socket session scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SocketSessionScope {
    /// Private to the current Linux uid and resumable.
    Private,
    /// Stored in a shared space when allowed by policy and mount visibility.
    Shared,
    /// Process-local session that need not survive socket close or agent exit.
    Temp,
}

impl SocketSessionScope {
    /// Parses a stable socket session scope.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "private" => Some(Self::Private),
            "shared" => Some(Self::Shared),
            "temp" => Some(Self::Temp),
            _ => None,
        }
    }

    /// Returns the stable scope word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Private => "private",
            Self::Shared => "shared",
            Self::Temp => "temp",
        }
    }
}

/// Canonical JSONL socket request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SocketRequest {
    /// Start or continue a run by sending user input into a session.
    Send {
        /// Client idempotency id.
        id: String,
        /// `CortexFS` session name. Defaults to `default` when omitted.
        session: String,
        /// Session storage scope. Defaults to `private` when omitted.
        scope: SocketSessionScope,
        /// Optional cwd inside the agent chroot.
        cwd: Option<String>,
        /// User input text.
        input: String,
    },
    /// Resume a session stream, optionally after an event id.
    Resume {
        /// `CortexFS` session name. Defaults to `default` when omitted.
        session: String,
        /// Optional event id cursor.
        after: Option<String>,
    },
    /// Cancel a run.
    Cancel {
        /// Run id to cancel.
        id: String,
    },
    /// Health check.
    Ping,
}

/// Stable socket request parse or validation error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SocketRequestError {
    /// Frame exceeds [`MAX_SOCKET_FRAME_BYTES`].
    FrameTooLarge { bytes: usize },
    /// Frame is empty after trimming one JSONL newline.
    EmptyFrame,
    /// Frame contains more than one non-empty JSONL line.
    MultipleFrames,
    /// Frame is not valid JSON.
    InvalidJson,
    /// JSON root is not an object.
    RequestNotObject,
    /// Request lacks an `op` string.
    MissingOp,
    /// Request `op` is not in the stable v1 request set.
    UnknownOp(String),
    /// Required string field is missing or not a string.
    MissingStringField(&'static str),
    /// Field is present but outside stable v1 syntax.
    InvalidField {
        /// Field name.
        field: &'static str,
        /// Rejected value.
        value: String,
    },
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
            | Self::CannotSocket => "EIO",
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

impl SocketRequestError {
    /// Returns a stable errno name for this parse failure.
    #[must_use]
    pub const fn errno(&self) -> &'static str {
        match *self {
            Self::FrameTooLarge { .. } => "EMSGSIZE",
            Self::EmptyFrame
            | Self::MultipleFrames
            | Self::InvalidJson
            | Self::RequestNotObject
            | Self::MissingOp
            | Self::UnknownOp(_)
            | Self::MissingStringField(_)
            | Self::InvalidField { .. } => "EINVAL",
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
        Self { root: root.into() }
    }

    /// Returns the backing root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Projects `getattr`.
    pub fn getattr(&self, abi_path: &str) -> Result<FuseV1Attr, FuseV1Error> {
        let path = self.resolve(abi_path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        Ok(FuseV1Attr::with_owner(
            normalize_fuse_abi_path(abi_path)?,
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
        let path = self.resolve(abi_path)?;
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
        let path = self.resolve(abi_path)?;
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
        let path = self.resolve(abi_path)?;
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

/// Parses one v1 JSONL socket request frame.
///
/// Unknown fields are ignored by design. Only the stable fields that affect
/// `CortexFS` session semantics are consumed.
pub fn parse_socket_request_frame(frame: &str) -> Result<SocketRequest, SocketRequestError> {
    if frame.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(SocketRequestError::FrameTooLarge { bytes: frame.len() });
    }

    let frame = trim_jsonl_frame(frame)?;
    let value =
        serde_json::from_str::<Value>(frame).map_err(|_error| SocketRequestError::InvalidJson)?;
    let object = value
        .as_object()
        .ok_or(SocketRequestError::RequestNotObject)?;
    let op = object
        .get("op")
        .and_then(Value::as_str)
        .ok_or(SocketRequestError::MissingOp)?;

    match op {
        "send" => parse_socket_send_request(object),
        "resume" => parse_socket_resume_request(object),
        "cancel" => parse_socket_cancel_request(object),
        "ping" => Ok(SocketRequest::Ping),
        other => Err(SocketRequestError::UnknownOp(other.to_owned())),
    }
}

fn trim_jsonl_frame(frame: &str) -> Result<&str, SocketRequestError> {
    let trimmed = frame.trim_end_matches(['\r', '\n']);
    if trimmed.trim().is_empty() {
        return Err(SocketRequestError::EmptyFrame);
    }
    if trimmed.lines().count() > 1 {
        return Err(SocketRequestError::MultipleFrames);
    }
    Ok(trimmed)
}

fn parse_socket_send_request(
    object: &serde_json::Map<String, Value>,
) -> Result<SocketRequest, SocketRequestError> {
    let id = required_socket_string(object, "id")?;
    validate_socket_object_field("id", id)?;
    let session = optional_socket_session(object)?;
    let scope = optional_socket_scope(object)?;
    let cwd = optional_socket_cwd(object)?;
    let input = required_socket_string(object, "input")?;
    if input.contains('\0') {
        return Err(SocketRequestError::InvalidField {
            field: "input",
            value: input.to_owned(),
        });
    }

    Ok(SocketRequest::Send {
        id: id.to_owned(),
        session,
        scope,
        cwd,
        input: input.to_owned(),
    })
}

fn parse_socket_resume_request(
    object: &serde_json::Map<String, Value>,
) -> Result<SocketRequest, SocketRequestError> {
    let session = optional_socket_session(object)?;
    let after = optional_socket_object_field(object, "after")?;
    Ok(SocketRequest::Resume { session, after })
}

fn parse_socket_cancel_request(
    object: &serde_json::Map<String, Value>,
) -> Result<SocketRequest, SocketRequestError> {
    let id = required_socket_string(object, "id")?;
    validate_socket_object_field("id", id)?;
    Ok(SocketRequest::Cancel { id: id.to_owned() })
}

fn required_socket_string<'a>(
    object: &'a serde_json::Map<String, Value>,
    field: &'static str,
) -> Result<&'a str, SocketRequestError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .ok_or(SocketRequestError::MissingStringField(field))
}

fn optional_socket_session(
    object: &serde_json::Map<String, Value>,
) -> Result<String, SocketRequestError> {
    match object.get("session") {
        None => Ok("default".to_owned()),
        Some(value) => {
            let Some(session) = value.as_str() else {
                return Err(SocketRequestError::MissingStringField("session"));
            };
            validate_socket_object_field("session", session)?;
            Ok(session.to_owned())
        }
    }
}

fn optional_socket_scope(
    object: &serde_json::Map<String, Value>,
) -> Result<SocketSessionScope, SocketRequestError> {
    match object.get("scope") {
        None => Ok(SocketSessionScope::Private),
        Some(value) => {
            let Some(scope) = value.as_str() else {
                return Err(SocketRequestError::MissingStringField("scope"));
            };
            SocketSessionScope::parse(scope).ok_or_else(|| SocketRequestError::InvalidField {
                field: "scope",
                value: scope.to_owned(),
            })
        }
    }
}

fn optional_socket_cwd(
    object: &serde_json::Map<String, Value>,
) -> Result<Option<String>, SocketRequestError> {
    match object.get("cwd") {
        None => Ok(None),
        Some(value) => {
            let Some(cwd) = value.as_str() else {
                return Err(SocketRequestError::MissingStringField("cwd"));
            };
            if !is_stable_chroot_absolute_path(cwd) {
                return Err(SocketRequestError::InvalidField {
                    field: "cwd",
                    value: cwd.to_owned(),
                });
            }
            Ok(Some(cwd.to_owned()))
        }
    }
}

fn optional_socket_object_field(
    object: &serde_json::Map<String, Value>,
    field: &'static str,
) -> Result<Option<String>, SocketRequestError> {
    match object.get(field) {
        None => Ok(None),
        Some(value) => {
            let Some(text) = value.as_str() else {
                return Err(SocketRequestError::MissingStringField(field));
            };
            validate_socket_object_field(field, text)?;
            Ok(Some(text.to_owned()))
        }
    }
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
        && !is_object_name(model)
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
    write_text_file_if_missing(
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
            Ok(())
        } else {
            Err(DurableSessionLayoutError::CannotCreate)
        };
    }
    if let Some(parent) = path.parent() {
        create_dir(parent)?;
    }
    fs::write(path, content).map_err(|_error| DurableSessionLayoutError::CannotCreate)
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
    match handle_agent_executable_socket_request_frame(runtime, &frame) {
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

fn handle_agent_executable_socket_request_frame(
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
        return handle_socket_request(
            runtime.session_root,
            runtime.default_cwd,
            runtime.model,
            &request,
        );
    };

    let recorder_response = handle_socket_request(
        runtime.session_root,
        runtime.default_cwd,
        runtime.model,
        &request,
    )?;
    let agent_output = run_agent_executable(
        runtime.agent_executable,
        runtime.agent_name,
        id,
        session,
        input,
    )?;
    let agent_frames = canonical_agent_event_frames(&agent_output)?;
    if scope != SocketSessionScope::Temp
        && let Some(text) = assistant_text_from_event_frames(&agent_frames)
    {
        let session_dir = runtime.session_root.join(session);
        record_assistant_response_to_session(&session_dir, id, &text)
            .map_err(SocketRuntimeError::Record)?;
    }

    let mut frames = recorder_response.frames().to_vec();
    frames.extend(
        agent_frames
            .into_iter()
            .filter(|line| event_type(line).as_deref() != Some("start")),
    );
    Ok(SocketRuntimeResponse::new(frames))
}

fn run_agent_executable(
    agent_executable: &Path,
    agent_name: &str,
    run_id: &str,
    session: &str,
    input: &str,
) -> Result<String, SocketRuntimeError> {
    let output = Command::new(agent_executable)
        .arg(input)
        .env("CTX_AGENT", agent_name)
        .env("CTX_RUN_ID", run_id)
        .env("CTX_SESSION", session)
        .output()
        .map_err(|_error| SocketRuntimeError::CannotRunAgent)?;
    if !output.status.success() {
        return Err(SocketRuntimeError::CannotRunAgent);
    }
    String::from_utf8(output.stdout).map_err(|_error| SocketRuntimeError::InvalidAgentOutput)
}

fn canonical_agent_event_frames(output: &str) -> Result<Vec<String>, SocketRuntimeError> {
    if !inspect_event_stream_jsonl(output).is_ok() {
        return Err(SocketRuntimeError::InvalidAgentOutput);
    }
    Ok(output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect())
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

/// Rebuilds `context/pack.json` and `context/pack.md` from session files.
///
/// The generated pack is derived state. It references only session-relative
/// sources that [`validate_context_pack_source`] accepts, includes recent raw
/// messages by reference, and may include child result channels but never child
/// full-history files.
pub fn rebuild_context_pack(
    session_dir: &Path,
    agent: Option<&str>,
    recent_message_limit: usize,
) -> Result<ContextPackBuild, ContextPackBuildError> {
    let session_name = session_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(ContextPackBuildError::InvalidSessionName)?;
    if !is_object_name(session_name) {
        return Err(ContextPackBuildError::InvalidSessionName);
    }
    if let Some(agent) = agent
        && !is_object_name(agent)
    {
        return Err(ContextPackBuildError::InvalidAgentName);
    }
    require_pack_session_files(session_dir)?;

    let context = session_dir.join("context");
    let budget = read_context_budget(&context.join("budget"))?;
    let messages = fs::read_to_string(session_dir.join("messages.jsonl"))
        .map_err(|_error| ContextPackBuildError::CannotRead)?;
    if !inspect_message_stream_jsonl(&messages).is_ok() {
        return Err(ContextPackBuildError::InvalidMessages);
    }

    let mut candidates = Vec::new();
    append_pinned_pack_candidates(&context, &mut candidates)?;
    append_context_file_candidate(
        &context,
        "summary",
        "context/summary.md",
        None,
        &mut candidates,
    )?;
    append_context_jsonl_candidate(
        &context,
        "facts",
        "context/facts.jsonl",
        ContextJsonlKind::Facts,
        &mut candidates,
    )?;
    append_context_jsonl_candidate(
        &context,
        "decisions",
        "context/decisions.jsonl",
        ContextJsonlKind::Decisions,
        &mut candidates,
    )?;
    append_context_file_candidate(&context, "todo", "context/todo.md", None, &mut candidates)?;
    append_context_jsonl_candidate(
        &context,
        "refs",
        "context/refs.jsonl",
        ContextJsonlKind::Refs,
        &mut candidates,
    )?;
    append_recent_messages_candidate(&messages, recent_message_limit, &mut candidates);
    append_child_result_candidates(&context, &mut candidates)?;

    let selected = select_pack_candidates(candidates, budget);
    let build = render_context_pack(session_name, agent, budget, &selected);
    if !inspect_context_pack_json(build.pack_json()).is_ok() {
        return Err(ContextPackBuildError::CannotRecord);
    }

    atomic_replace_text(&context.join("pack.json"), build.pack_json())
        .map_err(|_error| ContextPackBuildError::CannotRecord)?;
    atomic_replace_text(&context.join("pack.md"), build.pack_md())
        .map_err(|_error| ContextPackBuildError::CannotRecord)?;

    Ok(build)
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct PackCandidate {
    kind: String,
    source: String,
    range: Option<String>,
    tokens: u64,
    content: String,
}

impl PackCandidate {
    fn new(kind: &str, source: &str, range: Option<String>, content: String) -> Self {
        Self {
            kind: kind.to_owned(),
            source: source.to_owned(),
            range,
            tokens: estimate_context_tokens(&content),
            content,
        }
    }

    fn item(&self) -> ContextPackBuiltItem {
        ContextPackBuiltItem::new(&self.kind, &self.source, self.range.clone(), self.tokens)
    }
}

fn require_pack_session_files(session_dir: &Path) -> Result<(), ContextPackBuildError> {
    for file in SESSION_REQUIRED_FILES {
        if !session_dir.join(file).is_file() {
            return Err(ContextPackBuildError::MissingSession);
        }
    }
    let context = session_dir.join("context");
    if !context.is_dir() {
        return Err(ContextPackBuildError::MissingSession);
    }
    for file in CONTEXT_REQUIRED_FILES {
        if !context.join(file).is_file() {
            return Err(ContextPackBuildError::MissingSession);
        }
    }
    for dir in CONTEXT_REQUIRED_DIRS {
        if !context.join(dir).is_dir() {
            return Err(ContextPackBuildError::MissingSession);
        }
    }
    Ok(())
}

fn read_context_budget(path: &Path) -> Result<Option<u64>, ContextPackBuildError> {
    let content = fs::read_to_string(path).map_err(|_error| ContextPackBuildError::CannotRead)?;
    let lines = content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    let Some(line) = lines.first().copied() else {
        return Ok(None);
    };
    if lines.len() != 1 {
        return Err(ContextPackBuildError::InvalidBudget);
    }
    let value = line.trim();
    if value.is_empty() || line != value {
        return Err(ContextPackBuildError::InvalidBudget);
    }
    value
        .parse::<u64>()
        .map(Some)
        .map_err(|_error| ContextPackBuildError::InvalidBudget)
}

fn append_pinned_pack_candidates(
    context: &Path,
    candidates: &mut Vec<PackCandidate>,
) -> Result<(), ContextPackBuildError> {
    let pinned = context.join("pinned");
    let mut names = directory_entry_names(&pinned)?;
    names.sort();
    for name in names {
        if !is_safe_relative_file_name(&name) {
            continue;
        }
        let path = pinned.join(&name);
        if !path.is_file() {
            continue;
        }
        let source = format!("context/pinned/{name}");
        append_context_file_candidate(context, "system", &source, None, candidates)?;
    }
    Ok(())
}

fn append_context_file_candidate(
    context_dir: &Path,
    kind: &str,
    source: &str,
    range: Option<String>,
    candidates: &mut Vec<PackCandidate>,
) -> Result<(), ContextPackBuildError> {
    validate_context_pack_source(source).map_err(|_error| ContextPackBuildError::CannotRead)?;
    let body = read_context_source(context_dir, source)?;
    if !body.trim().is_empty() {
        candidates.push(PackCandidate::new(kind, source, range, body));
    }
    Ok(())
}

fn append_context_jsonl_candidate(
    context_dir: &Path,
    kind: &str,
    source: &str,
    jsonl_kind: ContextJsonlKind,
    candidates: &mut Vec<PackCandidate>,
) -> Result<(), ContextPackBuildError> {
    validate_context_pack_source(source).map_err(|_error| ContextPackBuildError::CannotRead)?;
    let body = read_context_source(context_dir, source)?;
    if body.trim().is_empty() {
        return Ok(());
    }
    if !inspect_context_jsonl(jsonl_kind, &body).is_ok() {
        return Err(ContextPackBuildError::InvalidContextJsonl);
    }
    candidates.push(PackCandidate::new(kind, source, None, body));
    Ok(())
}

fn append_recent_messages_candidate(
    messages: &str,
    recent_message_limit: usize,
    candidates: &mut Vec<PackCandidate>,
) {
    if recent_message_limit == 0 {
        return;
    }
    let lines = messages
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    if lines.is_empty() {
        return;
    }
    let start = lines.len().saturating_sub(recent_message_limit);
    let selected = lines
        .get(start..)
        .map_or_else(String::new, |tail| tail.join("\n"));
    let range = format!("tail:{}", lines.len() - start);
    candidates.push(PackCandidate::new(
        "recent_messages",
        "messages.jsonl",
        Some(range),
        format!("{selected}\n"),
    ));
}

fn append_child_result_candidates(
    context: &Path,
    candidates: &mut Vec<PackCandidate>,
) -> Result<(), ContextPackBuildError> {
    let child_root = context.join("child");
    let mut names = directory_entry_names(&child_root)?;
    names.sort();
    for child in names {
        if !is_object_name(&child) {
            return Err(ContextPackBuildError::InvalidChildName);
        }
        let child_dir = child_root.join(&child);
        if !child_dir.is_dir() {
            continue;
        }
        let result_source = format!("context/child/{child}/result.md");
        append_context_file_candidate(context, "child_result", &result_source, None, candidates)?;

        let refs_source = format!("context/child/{child}/refs.jsonl");
        let refs = read_context_source(context, &refs_source)?;
        if !refs.trim().is_empty() {
            if !inspect_context_jsonl(ContextJsonlKind::Refs, &refs).is_ok() {
                return Err(ContextPackBuildError::InvalidContextJsonl);
            }
            candidates.push(PackCandidate::new("child_refs", &refs_source, None, refs));
        }
    }
    Ok(())
}

fn read_context_source(context: &Path, source: &str) -> Result<String, ContextPackBuildError> {
    let relative = source
        .strip_prefix("context/")
        .ok_or(ContextPackBuildError::CannotRead)?;
    fs::read_to_string(context.join(relative)).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ContextPackBuildError::MissingSession
        } else {
            ContextPackBuildError::CannotRead
        }
    })
}

fn select_pack_candidates(
    candidates: Vec<PackCandidate>,
    budget: Option<u64>,
) -> Vec<PackCandidate> {
    let Some(limit) = budget.filter(|limit| *limit > 0) else {
        return candidates;
    };
    let mut used = 0_u64;
    let mut selected = Vec::new();
    for candidate in candidates {
        if used.saturating_add(candidate.tokens) <= limit {
            used = used.saturating_add(candidate.tokens);
            selected.push(candidate);
        }
    }
    selected
}

fn render_context_pack(
    session: &str,
    agent: Option<&str>,
    budget: Option<u64>,
    candidates: &[PackCandidate],
) -> ContextPackBuild {
    let items = candidates
        .iter()
        .map(PackCandidate::item)
        .collect::<Vec<_>>();
    let json_items = items.iter().map(context_pack_item_json).collect::<Vec<_>>();
    let mut pack = serde_json::Map::new();
    pack.insert("session".to_owned(), serde_json::json!(session));
    pack.insert("items".to_owned(), serde_json::json!(json_items));
    if let Some(agent) = agent {
        pack.insert("agent".to_owned(), serde_json::json!(agent));
    }
    if let Some(budget) = budget {
        pack.insert("budget_tokens".to_owned(), serde_json::json!(budget));
    }

    let pack_json = format!("{}\n", Value::Object(pack));
    let pack_md = render_context_pack_markdown(session, agent, budget, candidates);
    ContextPackBuild::new(items, pack_json, pack_md)
}

fn context_pack_item_json(item: &ContextPackBuiltItem) -> Value {
    let mut object = serde_json::Map::new();
    object.insert("kind".to_owned(), serde_json::json!(item.kind()));
    object.insert("source".to_owned(), serde_json::json!(item.source()));
    object.insert("tokens".to_owned(), serde_json::json!(item.tokens()));
    if let Some(range) = item.range() {
        object.insert("range".to_owned(), serde_json::json!(range));
    }
    Value::Object(object)
}

fn render_context_pack_markdown(
    session: &str,
    agent: Option<&str>,
    budget: Option<u64>,
    candidates: &[PackCandidate],
) -> String {
    let mut output = String::new();
    output.push_str("# CortexFS Context Pack\n\n");
    let _ = writeln!(output, "session: {session}");
    if let Some(agent) = agent {
        let _ = writeln!(output, "agent: {agent}");
    }
    if let Some(budget) = budget {
        let _ = writeln!(output, "budget_tokens: {budget}");
    }
    output.push('\n');

    for candidate in candidates {
        let _ = write!(output, "## {}\n\n", candidate.kind);
        let _ = writeln!(output, "source: {}", candidate.source);
        if let Some(range) = candidate.range.as_deref() {
            let _ = writeln!(output, "range: {range}");
        }
        let _ = write!(output, "tokens: {}\n\n", candidate.tokens);
        output.push_str("```text\n");
        output.push_str(&candidate.content);
        if !candidate.content.ends_with('\n') {
            output.push('\n');
        }
        output.push_str("```\n\n");
    }

    output
}

fn directory_entry_names(path: &Path) -> Result<Vec<String>, ContextPackBuildError> {
    let entries = fs::read_dir(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ContextPackBuildError::MissingSession
        } else {
            ContextPackBuildError::CannotRead
        }
    })?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_error| ContextPackBuildError::CannotRead)?;
        let name = entry
            .file_name()
            .to_str()
            .ok_or(ContextPackBuildError::CannotRead)?
            .to_owned();
        names.push(name);
    }
    Ok(names)
}

fn is_safe_relative_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.contains('/')
        && !name.contains('\0')
        && !name.contains('\t')
        && !name.contains('\n')
        && name != "."
        && name != ".."
}

fn estimate_context_tokens(content: &str) -> u64 {
    let words = content.split_whitespace().count();
    u64::try_from(words.max(1)).unwrap_or(u64::MAX)
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

/// Error while claiming a shared queue job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedQueueClaimError {
    /// Worker name is not a valid object-like token.
    InvalidWorkerName,
    /// Queue `pending/` cannot be read.
    CannotReadPending,
    /// A pending job entry could not be inspected.
    CannotInspectJob,
    /// Another worker already claimed the job or claim directory cannot be created.
    CannotCreateClaim,
    /// Pending job could not be moved into its claim directory.
    CannotClaimJob,
    /// The claimed job could not be recorded in `lease/`.
    CannotRecordLease,
}

/// Error while finishing a shared queue job.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedQueueFinishError {
    /// Job name is not a valid object-like token.
    InvalidJobName,
    /// Result could not be written into `done/` or `failed/`.
    CannotWriteResult,
    /// Claimed job file could not be moved into `done/` or `failed/`.
    CannotMoveClaimedJob,
    /// Claim or lease cleanup failed.
    CannotCleanup,
}

/// Error while recovering a shared queue job claim.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedQueueRecoverError {
    /// Job name is not a valid object-like token.
    InvalidJobName,
    /// No claimed job file exists to recover.
    MissingClaim,
    /// No lease exists for the claimed job.
    MissingLease,
    /// Claimed job could not be moved back into `pending/`.
    CannotRequeue,
    /// Claim or lease cleanup failed.
    CannotCleanup,
}

/// Terminal shared queue outcome directory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SharedQueueOutcome {
    /// Successful job result under `done/`.
    Done,
    /// Failed job result under `failed/`.
    Failed,
}

impl SharedQueueOutcome {
    /// Returns the stable queue directory for this outcome.
    #[must_use]
    pub const fn as_dir(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }
}

/// Claimed shared queue job.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SharedQueueClaim {
    job_name: String,
    claimed_path: PathBuf,
    lease_path: PathBuf,
}

impl SharedQueueClaim {
    /// Creates a claimed job record.
    #[must_use]
    pub fn new(job_name: String, claimed_path: PathBuf, lease_path: PathBuf) -> Self {
        Self {
            job_name,
            claimed_path,
            lease_path,
        }
    }

    /// Returns the claimed job file name.
    #[must_use]
    pub fn job_name(&self) -> &str {
        &self.job_name
    }

    /// Returns the claimed job path.
    #[must_use]
    pub fn claimed_path(&self) -> &Path {
        &self.claimed_path
    }

    /// Returns the recoverable lease directory path.
    #[must_use]
    pub fn lease_path(&self) -> &Path {
        &self.lease_path
    }
}

/// Fixed mount access mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MountMode {
    /// Read-only mount.
    ReadOnly,
    /// Read-write mount.
    ReadWrite,
}

impl MountMode {
    /// Parses `ro` or `rw`.
    pub fn parse(value: &str) -> Result<Self, MountError> {
        match value {
            "ro" => Ok(Self::ReadOnly),
            "rw" => Ok(Self::ReadWrite),
            _ => Err(MountError::InvalidMode),
        }
    }
}

/// Fixed v0 mount options.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MountOption {
    /// Bind mount one path.
    Bind,
    /// Recursive bind mount.
    RecursiveBind,
    /// Disable set-user-ID and set-group-ID behavior.
    NoSuid,
    /// Do not interpret character or block devices.
    NoDev,
    /// Do not execute files.
    NoExec,
}

impl MountOption {
    /// Parses one fixed v0 mount option.
    pub fn parse(value: &str) -> Result<Self, MountError> {
        match value {
            "bind" => Ok(Self::Bind),
            "rbind" => Ok(Self::RecursiveBind),
            "nosuid" => Ok(Self::NoSuid),
            "nodev" => Ok(Self::NoDev),
            "noexec" => Ok(Self::NoExec),
            _ => Err(MountError::InvalidOption),
        }
    }
}

/// One v0 mount table entry.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MountEntry {
    source: String,
    target: String,
    mode: MountMode,
    options: Vec<MountOption>,
}

/// One executable tool found through `CTX_PATH`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolHit {
    path: PathBuf,
    control_dir: PathBuf,
}

impl ToolHit {
    /// Creates a tool lookup hit and derives the matching `.d/` control dir.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        let control_dir = sibling_control_dir(&path);
        Self { path, control_dir }
    }

    /// Returns the executable tool path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the matching `.d/` control directory for this exact executable.
    #[must_use]
    pub fn control_dir(&self) -> &Path {
        &self.control_dir
    }
}

/// Agent/tool search path for executable capability endpoints.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolPath {
    dirs: Vec<PathBuf>,
}

impl ToolPath {
    /// Builds a `CTX_PATH` from already split directories.
    #[must_use]
    pub fn new(dirs: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            dirs: dirs.into_iter().collect(),
        }
    }

    /// Parses the Unix `CTX_PATH` form. Empty components are ignored so the
    /// current working directory is never implicitly a tool directory.
    #[must_use]
    pub fn parse(value: &str) -> Self {
        Self::new(
            value
                .split(':')
                .filter(|component| !component.is_empty())
                .map(PathBuf::from),
        )
    }

    /// Returns the v1 default path: global tools first, then user tools.
    #[must_use]
    pub fn default(root: &Path, home: &Path) -> Self {
        Self::new([root.join("tool"), home.join("tool")])
    }

    /// Returns search directories in left-to-right lookup order.
    #[must_use]
    pub fn dirs(&self) -> &[PathBuf] {
        &self.dirs
    }

    /// Finds the first executable file matching `name`.
    pub fn find(&self, name: &str) -> Result<Option<ToolHit>, ToolPathError> {
        if !is_object_name(name) {
            return Err(ToolPathError::InvalidName);
        }

        for dir in &self.dirs {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Ok(Some(ToolHit::new(candidate)));
            }
        }

        Ok(None)
    }

    /// Lists executable tool hits in lookup order. Non-executable files,
    /// sockets, and control directories are not hits.
    pub fn list(&self) -> Result<Vec<ToolHit>, ToolPathError> {
        let mut hits = Vec::new();
        for dir in &self.dirs {
            append_tool_hits(dir, &mut hits)?;
        }
        Ok(hits)
    }
}

fn append_tool_hits(dir: &Path, hits: &mut Vec<ToolHit>) -> Result<(), ToolPathError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_error) => return Err(ToolPathError::CannotReadDirectory),
    };

    let mut local = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_error| ToolPathError::CannotReadDirectory)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_object_name(&name) {
            let path = entry.path();
            if is_executable_file(&path) {
                local.push(ToolHit::new(path));
            }
        }
    }
    local.sort_by(|left, right| left.path.cmp(&right.path));
    hits.extend(local);
    Ok(())
}

fn sibling_control_dir(path: &Path) -> PathBuf {
    let mut control = path.as_os_str().to_owned();
    control.push(".d");
    PathBuf::from(control)
}

/// Returns whether the path is an executable regular file.
#[must_use]
pub fn is_executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

/// Inspects a `model/<name>.d/cap` file body for stable v1 capability words.
#[must_use]
pub fn inspect_model_capabilities(content: &str) -> ModelCapabilityReport {
    let mut issues = Vec::new();
    for (index, raw_line) in content.lines().enumerate() {
        let line = index + 1;
        let capability = raw_line.trim();
        if capability.is_empty() {
            continue;
        }
        if FORBIDDEN_MODEL_CAPABILITIES.contains(&capability) {
            issues.push(ModelCapabilityIssue::ProviderPrivate {
                line,
                capability: capability.to_owned(),
            });
        } else if !STABLE_MODEL_CAPABILITIES.contains(&capability) {
            issues.push(ModelCapabilityIssue::Unknown {
                line,
                capability: capability.to_owned(),
            });
        }
    }
    ModelCapabilityReport::new(issues)
}

/// Inspects a `tool/<name>.d/schema` file body.
#[must_use]
pub fn inspect_tool_schema_json(content: &str) -> ToolSchemaReport {
    let Ok(value) = serde_json::from_str::<Value>(content) else {
        return ToolSchemaReport::new(vec![ToolSchemaIssue::InvalidJson]);
    };
    let Some(object) = value.as_object() else {
        return ToolSchemaReport::new(vec![ToolSchemaIssue::NotObject]);
    };

    let issues = object
        .keys()
        .filter(|field| is_tool_schema_authority_field(field))
        .map(|field| ToolSchemaIssue::AuthorityField(field.clone()))
        .collect();
    ToolSchemaReport::new(issues)
}

fn is_tool_schema_authority_field(field: &str) -> bool {
    matches!(
        field,
        "policy"
            | "allow"
            | "deny"
            | "authority"
            | "grant"
            | "grants"
            | "permissions"
            | "capability_grants"
            | "mount"
            | "uid"
            | "gid"
            | "groups"
            | "network"
    )
}

/// Inspects a fixed-format v1 session index file.
#[must_use]
pub fn inspect_session_index(kind: SessionIndexKind, content: &str) -> SessionIndexReport {
    match kind {
        SessionIndexKind::List => inspect_session_index_list(content),
        SessionIndexKind::Current | SessionIndexKind::ByCwd => {
            inspect_single_session_index_value(content)
        }
    }
}

fn inspect_session_index_list(content: &str) -> SessionIndexReport {
    let mut issues = Vec::new();
    for (index, raw_line) in content.lines().enumerate() {
        inspect_session_index_name(index + 1, raw_line, &mut issues);
    }
    SessionIndexReport::new(issues)
}

fn inspect_single_session_index_value(content: &str) -> SessionIndexReport {
    let mut issues = Vec::new();
    let lines = content.lines().collect::<Vec<_>>();
    if let Some(first) = lines.first() {
        inspect_session_index_name(1, first, &mut issues);
        if lines.len() > 1 {
            issues.push(SessionIndexIssue::MultipleValues { line: 2 });
        }
    } else {
        issues.push(SessionIndexIssue::EmptyValue { line: 1 });
    }
    SessionIndexReport::new(issues)
}

fn inspect_session_index_name(line: usize, raw_line: &str, issues: &mut Vec<SessionIndexIssue>) {
    let value = raw_line.trim();
    if value.is_empty() {
        issues.push(SessionIndexIssue::EmptyValue { line });
    } else if value != raw_line || !is_object_name(value) {
        issues.push(SessionIndexIssue::InvalidSessionName {
            line,
            value: value.to_owned(),
        });
    }
}

/// Updates the reserved durable session index files for a selected session.
///
/// This rewrites `index/current`, de-duplicates and prepends the session in
/// `index/list`, and optionally writes `index/by-cwd/<key>`. The caller owns
/// deriving a stable `by-cwd` key from a cwd; this function only preserves the
/// fixed index file formats.
pub fn update_session_index(
    session_root: &Path,
    session_name: &str,
    by_cwd_key: Option<&str>,
) -> Result<(), SessionIndexUpdateError> {
    if !is_object_name(session_name) {
        return Err(SessionIndexUpdateError::InvalidSessionName);
    }
    if !session_root.join(session_name).is_dir() {
        return Err(SessionIndexUpdateError::MissingSession);
    }
    let index_dir = session_root.join("index");
    let list_path = index_dir.join("list");
    let current_path = index_dir.join("current");
    if !index_dir.is_dir() || !list_path.is_file() || !current_path.is_file() {
        return Err(SessionIndexUpdateError::MissingIndex);
    }
    let by_cwd_path = if let Some(key) = by_cwd_key {
        if !is_object_name(key) {
            return Err(SessionIndexUpdateError::InvalidByCwdKey);
        }
        let by_cwd_dir = index_dir.join("by-cwd");
        if !by_cwd_dir.is_dir() {
            return Err(SessionIndexUpdateError::MissingIndex);
        }
        Some(by_cwd_dir.join(key))
    } else {
        None
    };

    let list =
        fs::read_to_string(&list_path).map_err(|_error| SessionIndexUpdateError::CannotRecord)?;
    if !inspect_session_index(SessionIndexKind::List, &list).is_ok() {
        return Err(SessionIndexUpdateError::InvalidIndex);
    }
    if !inspect_session_index(
        SessionIndexKind::Current,
        &fs::read_to_string(&current_path)
            .map_err(|_error| SessionIndexUpdateError::CannotRecord)?,
    )
    .is_ok()
    {
        return Err(SessionIndexUpdateError::InvalidIndex);
    }

    let mut sessions = vec![session_name.to_owned()];
    sessions.extend(
        list.lines()
            .filter(|existing| *existing != session_name)
            .map(str::to_owned),
    );
    atomic_replace_text(&list_path, &format!("{}\n", sessions.join("\n")))
        .map_err(|_error| SessionIndexUpdateError::CannotRecord)?;
    atomic_replace_text(&current_path, &format!("{session_name}\n"))
        .map_err(|_error| SessionIndexUpdateError::CannotRecord)?;

    if let Some(path) = by_cwd_path {
        atomic_replace_text(&path, &format!("{session_name}\n"))
            .map_err(|_error| SessionIndexUpdateError::CannotRecord)?;
    }

    Ok(())
}

/// Inspects a fixed-format v1 agent control file body.
#[must_use]
pub fn inspect_agent_control(kind: AgentControlKind, content: &str) -> AgentControlReport {
    match kind {
        AgentControlKind::Groups => inspect_agent_groups_control(content),
        AgentControlKind::Parent => inspect_optional_agent_parent_control(content),
        AgentControlKind::Pid => inspect_optional_agent_number_control(content),
        AgentControlKind::Owner | AgentControlKind::Uid | AgentControlKind::Gid => {
            inspect_required_agent_number_control(content)
        }
        AgentControlKind::Iso | AgentControlKind::Life | AgentControlKind::Status => {
            inspect_agent_vocab_control(kind, content)
        }
    }
}

fn inspect_required_agent_number_control(content: &str) -> AgentControlReport {
    inspect_single_agent_control_value(content, true, |line, value, issues| {
        if value.parse::<u32>().is_err() {
            issues.push(AgentControlIssue::InvalidNumber {
                line,
                value: value.to_owned(),
            });
        }
    })
}

fn inspect_optional_agent_number_control(content: &str) -> AgentControlReport {
    inspect_single_agent_control_value(content, false, |line, value, issues| {
        if !value.is_empty() && value.parse::<u32>().is_err() {
            issues.push(AgentControlIssue::InvalidNumber {
                line,
                value: value.to_owned(),
            });
        }
    })
}

fn inspect_agent_groups_control(content: &str) -> AgentControlReport {
    let mut issues = Vec::new();
    for (index, raw_line) in content.lines().enumerate() {
        let line = index + 1;
        let value = raw_line.trim();
        if value.is_empty() {
            issues.push(AgentControlIssue::EmptyValue);
        } else if value != raw_line || value.parse::<u32>().is_err() {
            issues.push(AgentControlIssue::InvalidNumber {
                line,
                value: value.to_owned(),
            });
        }
    }
    AgentControlReport::new(issues)
}

fn inspect_optional_agent_parent_control(content: &str) -> AgentControlReport {
    inspect_single_agent_control_value(content, false, |line, value, issues| {
        if !value.is_empty() && parent_ref_agent_name(value).is_err() {
            issues.push(AgentControlIssue::InvalidValue {
                line,
                value: value.to_owned(),
            });
        }
    })
}

fn inspect_agent_vocab_control(kind: AgentControlKind, content: &str) -> AgentControlReport {
    inspect_single_agent_control_value(content, true, |line, value, issues| {
        if !agent_vocab_allows(kind, value) {
            issues.push(AgentControlIssue::InvalidValue {
                line,
                value: value.to_owned(),
            });
        }
    })
}

fn inspect_single_agent_control_value(
    content: &str,
    required: bool,
    validate: impl Fn(usize, &str, &mut Vec<AgentControlIssue>),
) -> AgentControlReport {
    let mut issues = Vec::new();
    let lines = content.lines().collect::<Vec<_>>();
    let value = lines.first().map_or("", |line| line.trim());
    if value.is_empty() {
        if required {
            issues.push(AgentControlIssue::EmptyValue);
        }
    } else if lines.first().is_some_and(|line| *line != value) {
        issues.push(AgentControlIssue::InvalidValue {
            line: 1,
            value: value.to_owned(),
        });
    } else {
        validate(1, value, &mut issues);
    }
    if lines.len() > 1 {
        issues.push(AgentControlIssue::MultipleValues { line: 2 });
    }
    AgentControlReport::new(issues)
}

fn agent_vocab_allows(kind: AgentControlKind, value: &str) -> bool {
    match kind {
        AgentControlKind::Iso => matches!(value, "shared" | "uid" | "userns"),
        AgentControlKind::Life => ChildLifecycle::parse(value).is_ok(),
        AgentControlKind::Status => {
            matches!(
                value,
                "start" | "ready" | "busy" | "idle" | "stopping" | "dead"
            )
        }
        AgentControlKind::Owner
        | AgentControlKind::Uid
        | AgentControlKind::Gid
        | AgentControlKind::Groups
        | AgentControlKind::Parent
        | AgentControlKind::Pid => false,
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
    if !is_object_name(&model) {
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
    if !is_object_name(name) {
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
        "session" if matches!(content.trim(), "none" | "socket") => Ok(()),
        "cap" | "session" => Err(ObjectBootstrapError::InvalidControlValue),
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
    ensure_reference_model(root)?;
    ensure_reference_agent(root, "coder")?;
    ensure_reference_agent(root, "reviewer")?;
    ensure_reference_global_tools(root)?;
    ensure_reference_home(root)?;
    ensure_reference_shared_project(root)?;
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

fn ensure_reference_model(root: &Path) -> Result<(), ReferenceTreeError> {
    install_executable_object_wrapper(
        root,
        ObjectClass::Model,
        "qwen",
        "/bin/false",
        &[
            ("id", "qwen"),
            ("driver", "rig"),
            ("cap", "chat\nstream\nsession\ntool_call_syntax"),
            ("default", ""),
            ("session", "socket"),
            ("status", "idle"),
            ("log", ""),
        ],
    )
    .map_err(ReferenceTreeError::Object)?;
    write_reference_text(
        &root.join("model").join("qwen"),
        reference_model_stub_script(),
    )?;
    set_reference_executable(&root.join("model").join("qwen"))?;
    ensure_reference_socket(&root.join("model").join("qwen.sock"))
}

fn reference_model_stub_script() -> &'static str {
    r#"#!/bin/sh
# CortexFS reference-tree model stub.
run="$CTX_RUN_ID"
if [ -z "$run" ]; then
  run="r1"
fi
input="$*"
if [ -z "$input" ]; then
  input="$(cat)"
fi
json_text="$(printf '%s' "$input" | sed 's/\\/\\\\/g; s/"/\\"/g')"
printf '{"type":"start","run":"%s","model":"qwen"}\n' "$run"
printf '{"type":"delta","run":"%s","text":"%s"}\n' "$run" "$json_text"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$run"
"#
}

fn ensure_reference_agent(root: &Path, name: &str) -> Result<(), ReferenceTreeError> {
    install_executable_object_wrapper(root, ObjectClass::Agent, name, "/bin/false", &[])
        .map_err(ReferenceTreeError::Object)?;
    let control = root.join("agent").join(format!("{name}.d"));
    let label = format!("user_u:agent_r:{name}_t:s0\n");
    let home_root = format!("/ctx/home/1000/agent/{name}/root\n");
    let policy_subject = format!("{name}_t");
    let policy = format!(
        "allow {policy_subject} model:qwen use\nallow {policy_subject} tool:fs.read execute\n"
    );
    let mount = format!(
        "/ctx\t/ctx\tro\trbind,nosuid,nodev\n/ctx/home/1000/agent/{name}\t/home/agent\trw\trbind,nosuid,nodev\n/ctx/shared/project-a\t/shared/project-a\trw\trbind,nosuid,nodev\n"
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
        (
            "path",
            "/ctx/tool:/ctx/home/1000/tool:/ctx/shared/project-a/tool\n".to_owned(),
        ),
        ("mount", mount),
        ("model", "qwen\n".to_owned()),
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
# CortexFS reference-tree agent stub.
run="${{CTX_RUN_ID:-r1}}"
input="$*"
if [ -z "$input" ]; then
  input="$(cat)"
fi
json_text="$(printf '%s' "$input" | sed 's/\\/\\\\/g; s/"/\\"/g')"
printf '{{"type":"start","run":"%s","agent":"{name}"}}\n' "$run"
printf '{{"type":"message","run":"%s","role":"assistant","content":[{{"type":"text","text":"%s"}}]}}\n' "$run" "$json_text"
printf '{{"type":"done","run":"%s","status":"ok"}}\n' "$run"
"#
    )
}

fn ensure_reference_global_tools(root: &Path) -> Result<(), ReferenceTreeError> {
    for tool in [
        "fs.read",
        "fs.write",
        "shell.exec",
        "mcp.github.search_issues",
        "agent.create",
        "agent.start",
        "agent.stop",
    ] {
        install_executable_object_wrapper(
            root,
            ObjectClass::Tool,
            tool,
            "/bin/false",
            &[
                ("name", tool),
                ("description", "CortexFS reference-tree tool"),
                ("schema", "{\"type\":\"object\"}"),
                ("cap", ""),
                ("policy", ""),
                ("status", "idle"),
                ("log", ""),
            ],
        )
        .map_err(ReferenceTreeError::Object)?;
        if let Some(script) = reference_tool_stub_script(tool) {
            write_reference_text(&root.join("tool").join(tool), script)?;
            set_reference_executable(&root.join("tool").join(tool))?;
        }
    }
    write_reference_text(
        &root
            .join("tool")
            .join("mcp.github.search_issues.d")
            .join("origin"),
        "mcp:github\n",
    )
}

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
    ensure_durable_session_layout(
        &agent_root.join("session"),
        "default",
        "/work",
        Some("qwen"),
        SocketSessionScope::Private,
    )
    .map_err(ReferenceTreeError::Session)?;
    let default_session = agent_root.join("session").join("default");
    record_child_handoff_to_parent_context(
        &default_session,
        "rev-123",
        "reviewer",
        "default",
        "Review the current design slice.",
    )
    .map_err(ReferenceTreeError::Child)?;
    record_child_result_to_parent_context(
        &default_session,
        "rev-123",
        ChildContextStatus::Done,
        "Reference tree child result placeholder.",
        "",
    )
    .map_err(ReferenceTreeError::Child)?;
    let cwd_key = session_index_key_for_cwd("/work").ok_or(ReferenceTreeError::CannotCreate)?;
    write_reference_text(
        &agent_root
            .join("session")
            .join("index")
            .join("by-cwd")
            .join(cwd_key),
        "default\n",
    )?;
    create_reference_dir(&agent_root.join("data"))?;
    create_reference_dir(&agent_root.join("cache"))?;
    create_reference_dir(&agent_root.join("log"))?;

    ensure_reference_symlink(
        &root.join("home").join("1000").join("tool").join("fs.read"),
        Path::new("/ctx/tool/fs.read"),
    )?;
    ensure_reference_symlink(
        &root.join("home").join("1000").join("model").join("coder"),
        Path::new("/ctx/model/qwen"),
    )
}

fn ensure_reference_shared_project(root: &Path) -> Result<(), ReferenceTreeError> {
    let project = root.join("shared").join("project-a");
    create_reference_dir(&project.join("data"))?;
    ensure_reference_project_tool(&project)?;
    ensure_durable_session_layout(
        &project.join("agent").join("coder").join("session"),
        "design-review",
        "/work",
        Some("qwen"),
        SocketSessionScope::Shared,
    )
    .map_err(ReferenceTreeError::Session)?;
    create_reference_dir(&project.join("queue"))?;
    for dir in SHARED_QUEUE_REQUIRED_DIRS {
        create_reference_dir(&project.join("queue").join(dir))?;
    }
    create_reference_dir(&project.join("result"))
}

fn ensure_reference_project_tool(project: &Path) -> Result<(), ReferenceTreeError> {
    let tool = project.join("tool").join("project.test");
    write_reference_text(
        &tool,
        "#!/bin/sh\n# CortexFS reference project tool placeholder.\nexit 0\n",
    )?;
    set_reference_executable(&tool)?;
    let control = project.join("tool").join("project.test.d");
    write_reference_text(&control.join("schema"), "{\"type\":\"object\"}\n")?;
    write_reference_text(&control.join("policy"), "\n")?;
    write_reference_text(&control.join("status"), "idle\n")?;
    write_reference_text(&control.join("log"), "\n")
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
        return if metadata.file_type().is_socket() {
            set_reference_socket_permissions(path)
        } else {
            Err(ReferenceTreeError::CannotSocket)
        };
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

fn ensure_reference_symlink(path: &Path, target: &Path) -> Result<(), ReferenceTreeError> {
    if let Ok(existing) = fs::read_link(path) {
        return if existing == target {
            Ok(())
        } else {
            Err(ReferenceTreeError::CannotLink)
        };
    }
    if path.exists() {
        return Err(ReferenceTreeError::CannotLink);
    }
    if let Some(parent) = path.parent() {
        create_reference_dir(parent)?;
    }
    symlink(target, path).map_err(|_error| ReferenceTreeError::CannotLink)
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
    if !is_object_name(name) {
        issues.push(ObjectLayoutIssue::MissingExecutable(format!(
            "{}/{}",
            class.as_str(),
            name
        )));
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
        ToolSchemaIssue::InvalidJson | ToolSchemaIssue::NotObject => "",
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
    match fs::symlink_metadata(path) {
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
    let Ok(value) = serde_json::from_str::<Value>(content) else {
        return SessionControlReport::new(vec![SessionControlIssue::InvalidJson]);
    };
    let Some(object) = value.as_object() else {
        return SessionControlReport::new(vec![SessionControlIssue::NotObject]);
    };

    let mut issues = Vec::new();
    inspect_optional_meta_string(object, "client", &mut issues, |_| true);
    inspect_optional_meta_string(object, "model", &mut issues, is_object_name);
    inspect_optional_meta_string(object, "scope", &mut issues, |scope| {
        matches!(scope, "private" | "shared" | "temp")
    });
    SessionControlReport::new(issues)
}

fn inspect_optional_meta_string(
    object: &serde_json::Map<String, Value>,
    field: &str,
    issues: &mut Vec<SessionControlIssue>,
    valid: impl Fn(&str) -> bool,
) {
    let Some(value) = object.get(field) else {
        return;
    };
    let Some(text) = value.as_str() else {
        issues.push(SessionControlIssue::InvalidValue {
            line: 1,
            value: field.to_owned(),
        });
        return;
    };
    if !valid(text) {
        issues.push(SessionControlIssue::InvalidValue {
            line: 1,
            value: text.to_owned(),
        });
    }
}

fn is_stable_chroot_absolute_path(value: &str) -> bool {
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

/// Inspects `context/pack.json` content for transparent, session-relative
/// source references.
#[must_use]
pub fn inspect_context_pack_json(content: &str) -> ContextPackReport {
    let Ok(value) = serde_json::from_str::<Value>(content) else {
        return ContextPackReport::new(vec![ContextPackIssue::InvalidJson]);
    };
    let Some(items) = value.get("items").and_then(Value::as_array) else {
        return ContextPackReport::new(vec![ContextPackIssue::ItemsNotArray]);
    };

    let mut issues = Vec::new();
    for (index, item) in items.iter().enumerate() {
        inspect_context_pack_item(index, item, &mut issues);
    }

    ContextPackReport::new(issues)
}

/// Inspects durable `messages.jsonl` for the canonical v1 role/content shape.
#[must_use]
pub fn inspect_message_stream_jsonl(content: &str) -> MessageStreamReport {
    let mut issues = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        inspect_message_stream_line(line_number, line, &mut issues);
    }
    MessageStreamReport::new(issues)
}

fn inspect_message_stream_line(
    line_number: usize,
    line: &str,
    issues: &mut Vec<MessageStreamIssue>,
) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        issues.push(MessageStreamIssue::InvalidJson(line_number));
        return;
    };
    let Some(object) = value.as_object() else {
        issues.push(MessageStreamIssue::MessageNotObject(line_number));
        return;
    };

    append_provider_native_message_field_issues(line_number, &value, issues);

    let Some(role) = object.get("role").and_then(Value::as_str) else {
        issues.push(MessageStreamIssue::MissingRole(line_number));
        return;
    };
    if !matches!(role, "system" | "user" | "assistant" | "tool") {
        issues.push(MessageStreamIssue::InvalidRole {
            line: line_number,
            role: role.to_owned(),
        });
    }

    let Some(content) = object.get("content") else {
        issues.push(MessageStreamIssue::MissingContent(line_number));
        return;
    };
    if !is_canonical_message_content(content) {
        issues.push(MessageStreamIssue::InvalidContent(line_number));
    }
}

fn is_canonical_message_content(value: &Value) -> bool {
    if value.as_str().is_some() {
        return true;
    }
    value
        .as_array()
        .is_some_and(|parts| parts.iter().all(is_canonical_message_content_part))
}

fn is_canonical_message_content_part(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    match object.get("type").and_then(Value::as_str) {
        Some("text") => object.get("text").and_then(Value::as_str).is_some(),
        Some("image") => object.get("path").and_then(Value::as_str).is_some(),
        Some("tool_result") => {
            object.get("tool_call_id").and_then(Value::as_str).is_some()
                && object
                    .get("content")
                    .is_some_and(is_canonical_message_content)
        }
        _ => false,
    }
}

fn append_provider_native_message_field_issues(
    line_number: usize,
    value: &Value,
    issues: &mut Vec<MessageStreamIssue>,
) {
    if let Some(object) = value.as_object() {
        for (key, child) in object {
            if is_provider_native_field(key) {
                issues.push(MessageStreamIssue::ProviderNativeField {
                    line: line_number,
                    field: key.clone(),
                });
            }
            append_provider_native_message_field_issues(line_number, child, issues);
        }
        return;
    }

    if let Some(items) = value.as_array() {
        for item in items {
            append_provider_native_message_field_issues(line_number, item, issues);
        }
    }
}

/// Inspects a stable context JSONL file body.
#[must_use]
pub fn inspect_context_jsonl(kind: ContextJsonlKind, content: &str) -> ContextJsonlReport {
    let mut issues = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        inspect_context_jsonl_line(kind, line_number, line, &mut issues);
    }
    ContextJsonlReport::new(issues)
}

fn inspect_context_jsonl_line(
    kind: ContextJsonlKind,
    line_number: usize,
    line: &str,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        issues.push(ContextJsonlIssue::InvalidJson(line_number));
        return;
    };
    let Some(object) = value.as_object() else {
        issues.push(ContextJsonlIssue::RecordNotObject(line_number));
        return;
    };

    match kind {
        ContextJsonlKind::Facts => inspect_fact_record(line_number, object, issues),
        ContextJsonlKind::Decisions => inspect_decision_record(line_number, object, issues),
        ContextJsonlKind::Refs => inspect_ref_record(line_number, object, issues),
        ContextJsonlKind::SwapIndex => inspect_swap_index_record(line_number, object, issues),
        ContextJsonlKind::DedupIndex => inspect_dedup_index_record(line_number, object, issues),
    }
}

fn inspect_fact_record(
    line: usize,
    object: &serde_json::Map<String, Value>,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(line, object, "id", issues, is_context_record_id);
    require_context_string_field(line, object, "text", issues, is_nonempty_single_line);
    require_context_string_field(line, object, "source", issues, is_nonempty_single_line);
}

fn inspect_decision_record(
    line: usize,
    object: &serde_json::Map<String, Value>,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(line, object, "id", issues, is_context_record_id);
    require_context_string_field(line, object, "decision", issues, is_nonempty_single_line);
    require_context_string_field(line, object, "source", issues, is_nonempty_single_line);
}

fn inspect_ref_record(
    line: usize,
    object: &serde_json::Map<String, Value>,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(line, object, "id", issues, is_context_record_id);
    require_context_string_field(line, object, "path", issues, is_stable_context_ref_path);
    require_context_string_field(line, object, "kind", issues, is_context_ref_kind);
    require_context_string_field(line, object, "summary", issues, is_nonempty_single_line);
}

fn inspect_swap_index_record(
    line: usize,
    object: &serde_json::Map<String, Value>,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(line, object, "id", issues, is_context_hash_id);
    require_context_string_field(line, object, "kind", issues, is_swap_kind);
    require_context_string_field(line, object, "source", issues, is_swap_source);
    require_context_string_field(line, object, "summary", issues, is_nonempty_single_line);
    require_context_number_field(line, object, "tokens", issues);
}

fn inspect_dedup_index_record(
    line: usize,
    object: &serde_json::Map<String, Value>,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(line, object, "hash", issues, is_context_hash_id);
    require_context_string_array_field(line, object, "refs", issues, is_nonempty_single_line);
    require_context_number_field(line, object, "bytes", issues);
    require_context_number_field(line, object, "tokens", issues);
}

fn require_context_string_field(
    line: usize,
    object: &serde_json::Map<String, Value>,
    field: &str,
    issues: &mut Vec<ContextJsonlIssue>,
    valid: impl Fn(&str) -> bool,
) {
    let Some(value) = object.get(field).and_then(Value::as_str) else {
        issues.push(ContextJsonlIssue::MissingStringField {
            line,
            field: field.to_owned(),
        });
        return;
    };
    if !valid(value) {
        issues.push(ContextJsonlIssue::InvalidField {
            line,
            field: field.to_owned(),
            value: value.to_owned(),
        });
    }
}

fn require_context_string_array_field(
    line: usize,
    object: &serde_json::Map<String, Value>,
    field: &str,
    issues: &mut Vec<ContextJsonlIssue>,
    valid: impl Fn(&str) -> bool,
) {
    let Some(values) = object.get(field).and_then(Value::as_array) else {
        issues.push(ContextJsonlIssue::MissingStringArrayField {
            line,
            field: field.to_owned(),
        });
        return;
    };
    if values.is_empty() {
        issues.push(ContextJsonlIssue::MissingStringArrayField {
            line,
            field: field.to_owned(),
        });
        return;
    }
    for value in values {
        let Some(text) = value.as_str() else {
            issues.push(ContextJsonlIssue::MissingStringArrayField {
                line,
                field: field.to_owned(),
            });
            return;
        };
        if !valid(text) {
            issues.push(ContextJsonlIssue::InvalidField {
                line,
                field: field.to_owned(),
                value: text.to_owned(),
            });
        }
    }
}

fn require_context_number_field(
    line: usize,
    object: &serde_json::Map<String, Value>,
    field: &str,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    if object.get(field).and_then(Value::as_u64).is_none() {
        issues.push(ContextJsonlIssue::MissingNumberField {
            line,
            field: field.to_owned(),
        });
    }
}

fn is_context_record_id(value: &str) -> bool {
    is_object_name(value)
}

fn is_context_hash_id(value: &str) -> bool {
    is_object_name(value)
        && (value.starts_with("sha256-")
            || value.starts_with("sha256_")
            || value.starts_with("sha256."))
}

fn is_nonempty_single_line(value: &str) -> bool {
    !value.is_empty() && !value.contains('\n') && !value.contains('\0')
}

fn is_stable_context_ref_path(value: &str) -> bool {
    is_nonempty_single_line(value)
        && !value.contains('\t')
        && !value.split('/').any(|part| part == "." || part == "..")
}

fn is_context_ref_kind(value: &str) -> bool {
    matches!(
        value,
        "file" | "artifact" | "tool_output" | "swap" | "child_result"
    )
}

fn is_swap_kind(value: &str) -> bool {
    matches!(value, "message_range" | "tool_output" | "file")
}

fn is_swap_source(value: &str) -> bool {
    matches!(value, "messages.jsonl" | "events.jsonl")
        || value.starts_with("context/")
            && validate_context_pack_source(value).is_ok()
            && !value.contains('\0')
}

/// Inspects a model or agent canonical JSONL event stream.
#[must_use]
pub fn inspect_event_stream_jsonl(content: &str) -> EventStreamReport {
    let mut issues = Vec::new();
    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        if line.trim().is_empty() {
            continue;
        }
        inspect_event_stream_line(line_number, line, &mut issues);
    }
    EventStreamReport::new(issues)
}

fn inspect_event_stream_line(line_number: usize, line: &str, issues: &mut Vec<EventStreamIssue>) {
    let Ok(value) = serde_json::from_str::<Value>(line) else {
        issues.push(EventStreamIssue::InvalidJson(line_number));
        return;
    };
    let Some(object) = value.as_object() else {
        issues.push(EventStreamIssue::EventNotObject(line_number));
        return;
    };

    append_provider_native_field_issues(line_number, &value, issues);

    let Some(event_type) = object.get("type").and_then(Value::as_str) else {
        issues.push(EventStreamIssue::MissingType(line_number));
        return;
    };
    if !is_canonical_event_type(event_type) {
        issues.push(EventStreamIssue::UnknownType {
            line: line_number,
            event_type: event_type.to_owned(),
        });
        return;
    }
    if event_requires_run(event_type) && object.get("run").and_then(Value::as_str).is_none() {
        issues.push(EventStreamIssue::MissingRun(line_number));
    }

    match event_type {
        "error" => inspect_error_event(line_number, object, issues),
        "done" => inspect_done_event(line_number, object, issues),
        "usage" => inspect_usage_event(line_number, object, issues),
        "tool_call" => inspect_tool_call_event(line_number, object, issues),
        "agent.child.cancel" => inspect_agent_child_cancel_event(line_number, object, issues),
        "agent.stop" => inspect_agent_stop_event(line_number, object, issues),
        _ => {}
    }
}

fn is_canonical_event_type(value: &str) -> bool {
    matches!(
        value,
        "start"
            | "delta"
            | "message"
            | "reasoning_delta"
            | "reasoning_message"
            | "tool_call"
            | "usage"
            | "error"
            | "done"
            | "agent.child.cancel"
            | "agent.stop"
    )
}

fn event_requires_run(event_type: &str) -> bool {
    !matches!(event_type, "agent.child.cancel" | "agent.stop")
}

fn append_provider_native_field_issues(
    line_number: usize,
    value: &Value,
    issues: &mut Vec<EventStreamIssue>,
) {
    if let Some(object) = value.as_object() {
        for (key, child) in object {
            if is_provider_native_field(key) {
                issues.push(EventStreamIssue::ProviderNativeField {
                    line: line_number,
                    field: key.clone(),
                });
            }
            append_provider_native_field_issues(line_number, child, issues);
        }
        return;
    }

    if let Some(items) = value.as_array() {
        for item in items {
            append_provider_native_field_issues(line_number, item, issues);
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

fn inspect_error_event(
    line_number: usize,
    object: &serde_json::Map<String, Value>,
    issues: &mut Vec<EventStreamIssue>,
) {
    let Some(code) = object.get("code").and_then(Value::as_str) else {
        issues.push(EventStreamIssue::InvalidErrorCode(line_number));
        return;
    };
    if !is_stable_errno(code) {
        issues.push(EventStreamIssue::InvalidErrorCode(line_number));
    }
}

fn inspect_done_event(
    line_number: usize,
    object: &serde_json::Map<String, Value>,
    issues: &mut Vec<EventStreamIssue>,
) {
    if !matches!(
        object.get("status").and_then(Value::as_str),
        Some("ok" | "error" | "cancelled")
    ) {
        issues.push(EventStreamIssue::InvalidDoneStatus(line_number));
    }
}

fn inspect_usage_event(
    line_number: usize,
    object: &serde_json::Map<String, Value>,
    issues: &mut Vec<EventStreamIssue>,
) {
    if object.get("input_tokens").and_then(Value::as_u64).is_none()
        || object
            .get("output_tokens")
            .and_then(Value::as_u64)
            .is_none()
    {
        issues.push(EventStreamIssue::InvalidUsage(line_number));
    }
}

fn inspect_tool_call_event(
    line_number: usize,
    object: &serde_json::Map<String, Value>,
    issues: &mut Vec<EventStreamIssue>,
) {
    let valid_id = object
        .get("id")
        .and_then(Value::as_str)
        .is_some_and(is_object_name);
    let valid_name = object
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(is_object_name);
    if !valid_id || !valid_name {
        issues.push(EventStreamIssue::InvalidToolCall(line_number));
    }
}

fn inspect_agent_child_cancel_event(
    line_number: usize,
    object: &serde_json::Map<String, Value>,
    issues: &mut Vec<EventStreamIssue>,
) {
    let parent = object.get("parent").and_then(Value::as_str);
    let child = object.get("child").and_then(Value::as_str);
    let reason = object.get("reason").and_then(Value::as_str);
    if !parent.is_some_and(is_object_name)
        || !child.is_some_and(is_object_name)
        || reason != Some("parent_dead")
    {
        issues.push(EventStreamIssue::InvalidAgentLifecycle(line_number));
    }
}

fn inspect_agent_stop_event(
    line_number: usize,
    object: &serde_json::Map<String, Value>,
    issues: &mut Vec<EventStreamIssue>,
) {
    let agent = object.get("agent").and_then(Value::as_str);
    let status = object.get("status").and_then(Value::as_str);
    if !agent.is_some_and(is_object_name) || status != Some("cancelled") {
        issues.push(EventStreamIssue::InvalidAgentLifecycle(line_number));
    }
}

fn is_stable_errno(code: &str) -> bool {
    matches!(
        code,
        "EACCES"
            | "EINVAL"
            | "ENOENT"
            | "EMSGSIZE"
            | "EHOSTDOWN"
            | "ECONNREFUSED"
            | "EAGAIN"
            | "EIO"
            | "EINTR"
            | "ENOSYS"
    )
}

fn inspect_context_pack_item(index: usize, item: &Value, issues: &mut Vec<ContextPackIssue>) {
    let Some(object) = item.as_object() else {
        issues.push(ContextPackIssue::ItemNotObject(index));
        return;
    };
    let Some(source) = object.get("source") else {
        issues.push(ContextPackIssue::MissingSource(index));
        return;
    };
    let Some(source) = source.as_str() else {
        issues.push(ContextPackIssue::SourceNotString(index));
        return;
    };
    if let Err(reason) = validate_context_pack_source(source) {
        issues.push(ContextPackIssue::InvalidSource {
            item: index,
            source: source.to_owned(),
            reason,
        });
    }
}

/// Returns whether a context pack source stays within the owning durable
/// session and does not include child full-history files.
pub fn validate_context_pack_source(source: &str) -> Result<(), ContextPackSourceError> {
    if source.is_empty() {
        return Err(ContextPackSourceError::Empty);
    }
    if source.starts_with('/') {
        return Err(ContextPackSourceError::Absolute);
    }

    let parts = parse_session_relative_source(source)?;
    if parts.len() == 1
        && parts
            .first()
            .is_some_and(|file| SESSION_REQUIRED_FILES.contains(file))
    {
        return Ok(());
    }
    if parts.first() == Some(&"context") {
        return if parts.get(1) == Some(&"child") {
            match (parts.get(2), parts.get(3..)) {
                (Some(child), Some(rest)) => validate_child_pack_source(child, rest),
                _ => Err(ContextPackSourceError::UnsupportedChildPath),
            }
        } else {
            Ok(())
        };
    }

    Err(ContextPackSourceError::UnsupportedSessionPath)
}

fn parse_session_relative_source(source: &str) -> Result<Vec<&str>, ContextPackSourceError> {
    let mut parts = Vec::new();
    for part in source.split('/') {
        if part.is_empty() {
            return Err(ContextPackSourceError::EmptyComponent);
        }
        if part == "." {
            return Err(ContextPackSourceError::DotComponent);
        }
        if part == ".." {
            return Err(ContextPackSourceError::ParentComponent);
        }
        parts.push(part);
    }
    Ok(parts)
}

fn validate_child_pack_source(child: &str, rest: &[&str]) -> Result<(), ContextPackSourceError> {
    if !is_object_name(child) {
        return Err(ContextPackSourceError::UnsupportedChildPath);
    }

    if rest.len() == 1
        && rest
            .first()
            .is_some_and(|file| matches!(*file, "handoff.md" | "result.md" | "refs.jsonl"))
    {
        return Ok(());
    }
    if rest.first() == Some(&"artifact") && rest.len() > 1 {
        return Ok(());
    }

    Err(ContextPackSourceError::UnsupportedChildPath)
}

/// Inspects a shared project queue for the v1 recommended directory shape.
#[must_use]
pub fn inspect_shared_queue_layout(queue_dir: &Path) -> SharedQueueLayoutReport {
    let mut issues = Vec::new();
    for dir in SHARED_QUEUE_REQUIRED_DIRS {
        require_shared_queue_directory(&queue_dir.join(dir), dir, &mut issues);
    }
    SharedQueueLayoutReport::new(issues)
}

/// Claims the first pending shared queue job using a mkdir lock plus atomic
/// rename into `claimed/`.
///
/// Pending entries are ordinary files under `pending/`. Invalid names and
/// non-files are ignored. The claimed job is moved to
/// `claimed/<job-name>/<job-name>`, and `lease/<job-name>/worker` records the
/// worker that claimed it. If another worker wins a race, this function skips
/// that job and tries the next pending entry.
pub fn claim_next_shared_queue_job(
    queue_dir: &Path,
    worker_name: &str,
) -> Result<Option<SharedQueueClaim>, SharedQueueClaimError> {
    if !is_object_name(worker_name) {
        return Err(SharedQueueClaimError::InvalidWorkerName);
    }

    let mut jobs = pending_queue_jobs(&queue_dir.join("pending"))?;
    jobs.sort_by(|left, right| left.0.cmp(&right.0));

    for (job_name, pending_path) in jobs {
        let claim_dir = queue_dir.join("claimed").join(&job_name);
        match fs::DirBuilder::new().create(&claim_dir) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_error) => return Err(SharedQueueClaimError::CannotCreateClaim),
        }

        let claimed_path = claim_dir.join(&job_name);
        match fs::rename(&pending_path, &claimed_path) {
            Ok(()) => {
                let lease_path = record_shared_queue_lease(queue_dir, &job_name, worker_name)?;
                return Ok(Some(SharedQueueClaim::new(
                    job_name,
                    claimed_path,
                    lease_path,
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let _ignored = fs::remove_dir(&claim_dir);
            }
            Err(_error) => return Err(SharedQueueClaimError::CannotClaimJob),
        }
    }

    Ok(None)
}

/// Finishes a claimed queue job by materializing a readable result file under
/// `done/` or `failed/`, moving the original claimed request beside it, and
/// removing the recoverable lease.
pub fn finish_shared_queue_job(
    queue_dir: &Path,
    job_name: &str,
    outcome: SharedQueueOutcome,
    result: &[u8],
) -> Result<PathBuf, SharedQueueFinishError> {
    if !is_object_name(job_name) {
        return Err(SharedQueueFinishError::InvalidJobName);
    }

    let output_dir = queue_dir.join(outcome.as_dir());
    let result_path = output_dir.join(format!("{job_name}.result"));
    let temp_path = output_dir.join(format!(".{job_name}.result.tmp"));
    fs::write(&temp_path, result).map_err(|_error| SharedQueueFinishError::CannotWriteResult)?;
    fs::rename(&temp_path, &result_path)
        .map_err(|_error| SharedQueueFinishError::CannotWriteResult)?;

    let claimed_file = claimed_queue_job_path(queue_dir, job_name);
    fs::rename(&claimed_file, output_dir.join(job_name))
        .map_err(|_error| SharedQueueFinishError::CannotMoveClaimedJob)?;
    cleanup_shared_queue_claim(queue_dir, job_name)
        .map_err(|_error| SharedQueueFinishError::CannotCleanup)?;

    Ok(result_path)
}

/// Recovers an explicitly abandoned claimed job by moving it back to
/// `pending/`. The existing `lease/<job>/worker` file is the durable evidence
/// that a worker previously claimed the job.
pub fn recover_shared_queue_job(
    queue_dir: &Path,
    job_name: &str,
) -> Result<PathBuf, SharedQueueRecoverError> {
    if !is_object_name(job_name) {
        return Err(SharedQueueRecoverError::InvalidJobName);
    }

    let claimed_file = claimed_queue_job_path(queue_dir, job_name);
    if !claimed_file.is_file() {
        return Err(SharedQueueRecoverError::MissingClaim);
    }
    if !queue_dir
        .join("lease")
        .join(job_name)
        .join("worker")
        .is_file()
    {
        return Err(SharedQueueRecoverError::MissingLease);
    }

    let pending_path = queue_dir.join("pending").join(job_name);
    fs::rename(&claimed_file, &pending_path)
        .map_err(|_error| SharedQueueRecoverError::CannotRequeue)?;
    cleanup_shared_queue_claim(queue_dir, job_name)
        .map_err(|_error| SharedQueueRecoverError::CannotCleanup)?;

    Ok(pending_path)
}

fn pending_queue_jobs(pending_dir: &Path) -> Result<Vec<(String, PathBuf)>, SharedQueueClaimError> {
    let entries =
        fs::read_dir(pending_dir).map_err(|_error| SharedQueueClaimError::CannotReadPending)?;
    let mut jobs = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_error| SharedQueueClaimError::CannotReadPending)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_object_name(&name) {
            continue;
        }
        let metadata = entry
            .metadata()
            .map_err(|_error| SharedQueueClaimError::CannotInspectJob)?;
        if metadata.is_file() {
            jobs.push((name, entry.path()));
        }
    }
    Ok(jobs)
}

fn record_shared_queue_lease(
    queue_dir: &Path,
    job_name: &str,
    worker_name: &str,
) -> Result<PathBuf, SharedQueueClaimError> {
    let lease_path = queue_dir.join("lease").join(job_name);
    fs::create_dir_all(&lease_path).map_err(|_error| SharedQueueClaimError::CannotRecordLease)?;
    fs::write(lease_path.join("worker"), newline_terminated(worker_name))
        .map_err(|_error| SharedQueueClaimError::CannotRecordLease)?;
    Ok(lease_path)
}

fn claimed_queue_job_path(queue_dir: &Path, job_name: &str) -> PathBuf {
    queue_dir.join("claimed").join(job_name).join(job_name)
}

fn cleanup_shared_queue_claim(queue_dir: &Path, job_name: &str) -> std::io::Result<()> {
    let claim_dir = queue_dir.join("claimed").join(job_name);
    let lease_dir = queue_dir.join("lease").join(job_name);
    match fs::remove_dir(&claim_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    match fs::remove_dir_all(&lease_dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(())
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

fn require_shared_queue_directory(
    path: &Path,
    label: &str,
    issues: &mut Vec<SharedQueueLayoutIssue>,
) {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_metadata) => issues.push(SharedQueueLayoutIssue::NotDirectory(label.to_owned())),
        Err(_error) => issues.push(SharedQueueLayoutIssue::MissingDirectory(label.to_owned())),
    }
}

fn newline_terminated(value: &str) -> String {
    if value.ends_with('\n') {
        value.to_owned()
    } else {
        format!("{value}\n")
    }
}

impl MountEntry {
    /// Parses `source<TAB>target<TAB>mode<TAB>options`.
    pub fn parse(line: &str) -> Result<Self, MountError> {
        let mut fields = line.split('\t');
        let Some(source) = fields.next() else {
            return Err(MountError::WrongFieldCount);
        };
        let Some(target) = fields.next() else {
            return Err(MountError::WrongFieldCount);
        };
        let Some(mode) = fields.next() else {
            return Err(MountError::WrongFieldCount);
        };
        let Some(options) = fields.next() else {
            return Err(MountError::WrongFieldCount);
        };
        if fields.next().is_some() {
            return Err(MountError::WrongFieldCount);
        }
        if !is_absolute_mount_path(source) || !is_absolute_mount_path(target) {
            return Err(MountError::InvalidPath);
        }

        let mode = MountMode::parse(mode)?;
        let options = parse_mount_options(options)?;
        Ok(Self {
            source: source.to_owned(),
            target: target.to_owned(),
            mode,
            options,
        })
    }

    /// Returns the source path.
    #[must_use]
    pub fn source(&self) -> &str {
        &self.source
    }

    /// Returns the target path.
    #[must_use]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Returns the mount mode.
    #[must_use]
    pub const fn mode(&self) -> MountMode {
        self.mode
    }

    /// Returns mount options.
    #[must_use]
    pub fn options(&self) -> &[MountOption] {
        &self.options
    }

    /// Returns whether this entry is no more permissive than `parent`.
    ///
    /// v0 requires the same source and target. A child may narrow `rw` to `ro`
    /// and may add safety options, but must not remove parent safety options or
    /// make bind traversal broader.
    #[must_use]
    pub fn is_subset_of(&self, parent: &Self) -> bool {
        self.source == parent.source
            && self.target == parent.target
            && mount_mode_allows(parent.mode, self.mode)
            && mount_options_allow(parent.options(), self.options())
    }
}

/// Parsed v0 mount table.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MountTable {
    entries: Vec<MountEntry>,
}

impl MountTable {
    /// Parses a v0 mount table.
    pub fn parse(content: &str) -> Result<Self, MountError> {
        let mut entries = Vec::new();
        for line in content.lines().filter(|line| !line.trim().is_empty()) {
            entries.push(MountEntry::parse(line)?);
        }
        Ok(Self { entries })
    }

    /// Returns parsed mount entries.
    #[must_use]
    pub fn entries(&self) -> &[MountEntry] {
        &self.entries
    }

    /// Returns whether every child mount is visible in `parent` with no
    /// expanded authority.
    #[must_use]
    pub fn is_subset_of(&self, parent: &Self) -> bool {
        self.entries.iter().all(|child| {
            parent
                .entries
                .iter()
                .any(|parent_entry| child.is_subset_of(parent_entry))
        })
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

fn parent_ref_agent_name(value: &str) -> Result<&str, ChildAgentDenial> {
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

fn atomic_replace_text(path: &Path, content: &str) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temp_path = parent.join(format!(".{file_name}.tmp.{}", std::process::id()));
    fs::write(&temp_path, content)?;
    fs::rename(&temp_path, path)
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

fn mount_mode_allows(parent: MountMode, child: MountMode) -> bool {
    matches!(
        (parent, child),
        (
            MountMode::ReadWrite,
            MountMode::ReadWrite | MountMode::ReadOnly
        ) | (MountMode::ReadOnly, MountMode::ReadOnly)
    )
}

fn mount_options_allow(parent: &[MountOption], child: &[MountOption]) -> bool {
    safety_options_preserved(parent, child) && bind_rank(child) <= bind_rank(parent)
}

fn safety_options_preserved(parent: &[MountOption], child: &[MountOption]) -> bool {
    [MountOption::NoSuid, MountOption::NoDev, MountOption::NoExec]
        .into_iter()
        .all(|option| !parent.contains(&option) || child.contains(&option))
}

fn bind_rank(options: &[MountOption]) -> u8 {
    if options.contains(&MountOption::RecursiveBind) {
        2
    } else {
        u8::from(options.contains(&MountOption::Bind))
    }
}

fn is_absolute_mount_path(value: &str) -> bool {
    value.starts_with('/') && !value.contains('\t') && !value.contains('\n')
}

fn parse_mount_options(value: &str) -> Result<Vec<MountOption>, MountError> {
    if value == "-" {
        return Ok(Vec::new());
    }
    let mut options = Vec::new();
    for option in value.split(',') {
        let option = MountOption::parse(option)?;
        if options.contains(&option) {
            return Err(MountError::DuplicateOption);
        }
        if matches!(option, MountOption::Bind) && options.contains(&MountOption::RecursiveBind)
            || matches!(option, MountOption::RecursiveBind) && options.contains(&MountOption::Bind)
        {
            return Err(MountError::ConflictingBindOption);
        }
        options.push(option);
    }
    Ok(options)
}

/// Fixed v1 policy object classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyObjectClass {
    /// Tool executable capability.
    Tool,
    /// Model inference endpoint.
    Model,
    /// Shared project or collaboration space.
    Shared,
    /// Durable session state.
    Session,
    /// Agent-visible mount.
    Mount,
    /// Agent object lifecycle or files.
    Agent,
    /// Network capability.
    Network,
}

impl PolicyObjectClass {
    /// Parses a fixed v1 policy object class.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "tool" => Some(Self::Tool),
            "model" => Some(Self::Model),
            "shared" => Some(Self::Shared),
            "session" => Some(Self::Session),
            "mount" => Some(Self::Mount),
            "agent" => Some(Self::Agent),
            "network" => Some(Self::Network),
            _ => None,
        }
    }
}

/// Fixed v1 policy permissions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PolicyPermission {
    /// Execute a tool.
    Execute,
    /// Use a model.
    Use,
    /// Read a file, session, mount, shared space, or agent state.
    Read,
    /// Write a file, session, mount, shared space, or agent state.
    Write,
    /// Resume a session.
    Resume,
    /// Create an agent.
    Create,
    /// Start an agent.
    Start,
    /// Stop an agent.
    Stop,
    /// Connect through a network capability.
    Connect,
}

impl PolicyPermission {
    /// Parses a permission that is valid for `class`.
    #[must_use]
    pub fn parse_for_class(class: PolicyObjectClass, value: &str) -> Option<Self> {
        match (class, value) {
            (PolicyObjectClass::Tool, "execute") => Some(Self::Execute),
            (PolicyObjectClass::Model, "use") => Some(Self::Use),
            (
                PolicyObjectClass::Shared
                | PolicyObjectClass::Session
                | PolicyObjectClass::Mount
                | PolicyObjectClass::Agent,
                "read",
            ) => Some(Self::Read),
            (
                PolicyObjectClass::Shared
                | PolicyObjectClass::Session
                | PolicyObjectClass::Mount
                | PolicyObjectClass::Agent,
                "write",
            ) => Some(Self::Write),
            (PolicyObjectClass::Session, "resume") => Some(Self::Resume),
            (PolicyObjectClass::Agent, "create") => Some(Self::Create),
            (PolicyObjectClass::Agent, "start") => Some(Self::Start),
            (PolicyObjectClass::Agent, "stop") => Some(Self::Stop),
            (PolicyObjectClass::Network, "connect") => Some(Self::Connect),
            _ => None,
        }
    }
}

/// One v0 allow rule.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyRule {
    subject_type: String,
    object_class: PolicyObjectClass,
    object_name: String,
    permission: PolicyPermission,
}

impl PolicyRule {
    /// Parses `allow <subject_type> <object_class>:<object_name> <permission>`.
    pub fn parse(line: &str) -> Result<Self, PolicyError> {
        let mut fields = line.split_whitespace();
        let Some(keyword) = fields.next() else {
            return Err(PolicyError::WrongFieldCount);
        };
        let Some(subject_type) = fields.next() else {
            return Err(PolicyError::WrongFieldCount);
        };
        let Some(object) = fields.next() else {
            return Err(PolicyError::WrongFieldCount);
        };
        let Some(permission) = fields.next() else {
            return Err(PolicyError::WrongFieldCount);
        };
        if fields.next().is_some() {
            return Err(PolicyError::WrongFieldCount);
        }
        if keyword != "allow" {
            return Err(PolicyError::ExpectedAllow);
        }
        if !is_object_name(subject_type) {
            return Err(PolicyError::InvalidName);
        }
        let (class, object_name) = object.split_once(':').ok_or(PolicyError::InvalidObject)?;
        if !is_object_name(object_name) {
            return Err(PolicyError::InvalidName);
        }
        let object_class = PolicyObjectClass::parse(class).ok_or(PolicyError::UnknownClass)?;
        let permission = PolicyPermission::parse_for_class(object_class, permission)
            .ok_or(PolicyError::UnknownPermission)?;

        Ok(Self {
            subject_type: subject_type.to_owned(),
            object_class,
            object_name: object_name.to_owned(),
            permission,
        })
    }

    /// Returns the subject type.
    #[must_use]
    pub fn subject_type(&self) -> &str {
        &self.subject_type
    }

    /// Returns the object class.
    #[must_use]
    pub const fn object_class(&self) -> PolicyObjectClass {
        self.object_class
    }

    /// Returns the object name.
    #[must_use]
    pub fn object_name(&self) -> &str {
        &self.object_name
    }

    /// Returns the permission.
    #[must_use]
    pub const fn permission(&self) -> PolicyPermission {
        self.permission
    }
}

/// Parsed v0 default-deny allowlist.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PolicyV0 {
    rules: Vec<PolicyRule>,
}

impl PolicyV0 {
    /// Parses a v0 policy file.
    pub fn parse(content: &str) -> Result<Self, PolicyError> {
        let mut rules = Vec::new();
        for line in content.lines().filter(|line| !line.trim().is_empty()) {
            rules.push(PolicyRule::parse(line)?);
        }
        Ok(Self { rules })
    }

    /// Returns whether a concrete request is allowed.
    #[must_use]
    pub fn allows(
        &self,
        subject_type: &str,
        object_class: PolicyObjectClass,
        object_name: &str,
        permission: PolicyPermission,
    ) -> bool {
        self.rules.iter().any(|rule| {
            rule.subject_type == subject_type
                && rule.object_class == object_class
                && rule.object_name == object_name
                && rule.permission == permission
        })
    }

    /// Returns the parsed allow rules.
    #[must_use]
    pub fn rules(&self) -> &[PolicyRule] {
        &self.rules
    }

    /// Returns whether every rule is also present in `parent` with the same
    /// subject, object, and permission.
    #[must_use]
    pub fn is_exact_subset_of(&self, parent: &Self) -> bool {
        self.rules.iter().all(|rule| {
            parent.allows(
                rule.subject_type(),
                rule.object_class(),
                rule.object_name(),
                rule.permission(),
            )
        })
    }

    /// Returns whether `child_subject` receives only authority that
    /// `parent_subject` already has.
    ///
    /// This is the v0 child-agent attenuation check. Child labels may differ
    /// from parent labels, so comparison maps each child rule to the parent
    /// subject while requiring object class, object name, and permission to
    /// match exactly.
    #[must_use]
    pub fn is_authority_subset_of(
        &self,
        parent: &Self,
        child_subject: &str,
        parent_subject: &str,
    ) -> bool {
        is_object_name(child_subject)
            && is_object_name(parent_subject)
            && self.rules.iter().all(|rule| {
                rule.subject_type() == child_subject
                    && parent.allows(
                        parent_subject,
                        rule.object_class(),
                        rule.object_name(),
                        rule.permission(),
                    )
            })
    }
}

/// Stable executable object classes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ObjectClass {
    /// Pure inference endpoint.
    Model,
    /// Policy-bound orchestrator endpoint.
    Agent,
    /// Executable capability endpoint.
    Tool,
}

impl ObjectClass {
    /// Parses a stable object class name.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "model" => Some(Self::Model),
            "agent" => Some(Self::Agent),
            "tool" => Some(Self::Tool),
            _ => None,
        }
    }

    /// Returns the ABI directory name for this object class.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Model => "model",
            Self::Agent => "agent",
            Self::Tool => "tool",
        }
    }

    /// Returns the stable `ctx file` type for an executable object.
    #[must_use]
    pub const fn exec_type(self) -> &'static str {
        match self {
            Self::Model => "ctx.model.exec",
            Self::Agent => "ctx.agent.exec",
            Self::Tool => "ctx.tool.exec",
        }
    }

    /// Returns the stable `ctx file` type for an object socket.
    #[must_use]
    pub const fn socket_type(self) -> &'static str {
        match self {
            Self::Model => "ctx.model.socket",
            Self::Agent => "ctx.agent.socket",
            Self::Tool => "ctx.tool.socket",
        }
    }

    /// Returns the stable `ctx file` type for an object control path.
    #[must_use]
    pub const fn control_type(self) -> &'static str {
        match self {
            Self::Model => "ctx.model.control",
            Self::Agent => "ctx.agent.control",
            Self::Tool => "ctx.tool.control",
        }
    }
}

/// Returns whether a top-level path component is part of the public ABI.
#[must_use]
pub fn is_root_entry(name: &str) -> bool {
    ROOT_ENTRIES.contains(&name)
}

/// Returns whether a model, agent, tool, session, or shared object name is valid.
#[must_use]
pub fn is_object_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_OBJECT_NAME_LEN {
        return false;
    }
    if name.strip_suffix(".sock").is_some() || name.strip_suffix(".d").is_some() {
        return false;
    }

    name.bytes().enumerate().all(|(index, byte)| {
        let is_extra = matches!(byte, b'.' | b'_' | b'+' | b'-');
        let valid = byte.is_ascii_alphanumeric() || is_extra;
        if index == 0 {
            byte.is_ascii_alphanumeric()
        } else {
            valid
        }
    })
}

/// Classifies a relative `CortexFS` ABI path by path shape.
#[must_use]
pub fn classify_abi_path(path: &str) -> &'static str {
    let trimmed = path.strip_prefix("./").map_or(path, |value| value);

    if trimmed.is_empty() || trimmed.split('/').any(str::is_empty) {
        return "ctx.unknown";
    }

    let mut parts = trimmed.split('/');
    let Some(first) = parts.next() else {
        return "ctx.unknown";
    };

    match ObjectClass::parse(first) {
        Some(class) => classify_object_path(class, parts),
        None => classify_non_object_path(first, parts),
    }
}

fn classify_object_path<'a>(
    class: ObjectClass,
    mut parts: impl Iterator<Item = &'a str>,
) -> &'static str {
    let Some(name) = parts.next() else {
        return "ctx.unknown";
    };

    if let Some(object_name) = name.strip_suffix(".sock") {
        if parts.next().is_some() {
            return "ctx.unknown";
        }
        if is_object_name(object_name) {
            return class.socket_type();
        }
        return "ctx.unknown";
    }

    if let Some(object_name) = name.strip_suffix(".d") {
        let has_control_path = parts.next().is_some();
        if is_object_name(object_name) {
            return if has_control_path {
                class.control_type()
            } else {
                "ctx.unknown"
            };
        }
        return "ctx.unknown";
    }

    if parts.next().is_some() {
        return "ctx.unknown";
    }

    if is_object_name(name) {
        class.exec_type()
    } else {
        "ctx.unknown"
    }
}

fn classify_non_object_path<'a>(first: &str, parts: impl Iterator<Item = &'a str>) -> &'static str {
    match first {
        "home" => classify_home_path(parts),
        "shared" => classify_shared_path(parts),
        _ => "ctx.unknown",
    }
}

fn classify_home_path<'a>(mut parts: impl Iterator<Item = &'a str>) -> &'static str {
    let Some(_uid) = parts.next() else {
        return "ctx.unknown";
    };

    match parts.next() {
        Some("agent") => classify_agent_home_path(parts),
        Some("model") => classify_model_home_path(parts),
        None | Some(_) => "ctx.home.dir",
    }
}

fn classify_agent_home_path<'a>(mut parts: impl Iterator<Item = &'a str>) -> &'static str {
    let Some(agent) = parts.next() else {
        return "ctx.home.dir";
    };
    if !is_object_name(agent) {
        return "ctx.unknown";
    }

    match parts.next() {
        Some("session") => classify_session_path(parts),
        None | Some(_) => "ctx.home.dir",
    }
}

fn classify_model_home_path<'a>(mut parts: impl Iterator<Item = &'a str>) -> &'static str {
    let Some(model_dir) = parts.next() else {
        return "ctx.home.dir";
    };
    let Some(model) = model_dir.strip_suffix(".d") else {
        return "ctx.home.dir";
    };
    if !is_object_name(model) {
        return "ctx.unknown";
    }

    match parts.next() {
        Some("session") => classify_session_path(parts),
        None | Some(_) => "ctx.home.dir",
    }
}

fn classify_shared_path<'a>(mut parts: impl Iterator<Item = &'a str>) -> &'static str {
    let Some(space) = parts.next() else {
        return "ctx.unknown";
    };
    if !is_object_name(space) {
        return "ctx.unknown";
    }

    match parts.next() {
        Some("agent") => classify_shared_agent_path(parts),
        Some("model") => classify_shared_model_path(parts),
        Some("tool") => classify_shared_tool_path(parts),
        Some("queue") => classify_shared_queue_path(parts),
        Some("result") => {
            if parts.next().is_none() {
                "ctx.shared.result"
            } else {
                "ctx.ordinary"
            }
        }
        None | Some(_) => "ctx.shared.dir",
    }
}

fn classify_shared_tool_path<'a>(mut parts: impl Iterator<Item = &'a str>) -> &'static str {
    let Some(name) = parts.next() else {
        return "ctx.shared.dir";
    };

    if let Some(tool_name) = name.strip_suffix(".d") {
        return if is_object_name(tool_name) && parts.next().is_some() {
            "ctx.shared.tool.control"
        } else {
            "ctx.unknown"
        };
    }

    if parts.next().is_none() && is_object_name(name) {
        "ctx.shared.tool.exec"
    } else {
        "ctx.unknown"
    }
}

fn classify_shared_queue_path<'a>(mut parts: impl Iterator<Item = &'a str>) -> &'static str {
    match parts.next() {
        None => "ctx.shared.queue",
        Some("inbox" | "pending" | "lease" | "claimed" | "done" | "failed")
            if parts.next().is_none() =>
        {
            "ctx.shared.queue"
        }
        Some(_) => "ctx.ordinary",
    }
}

fn classify_shared_agent_path<'a>(mut parts: impl Iterator<Item = &'a str>) -> &'static str {
    let Some(agent) = parts.next() else {
        return "ctx.shared.dir";
    };
    if !is_object_name(agent) {
        return "ctx.unknown";
    }

    match parts.next() {
        Some("session") => classify_session_path(parts),
        None | Some(_) => "ctx.shared.dir",
    }
}

fn classify_shared_model_path<'a>(mut parts: impl Iterator<Item = &'a str>) -> &'static str {
    let Some(model_dir) = parts.next() else {
        return "ctx.shared.dir";
    };
    let Some(model) = model_dir.strip_suffix(".d") else {
        return "ctx.shared.dir";
    };
    if !is_object_name(model) {
        return "ctx.unknown";
    }

    match parts.next() {
        Some("session") => classify_session_path(parts),
        None | Some(_) => "ctx.shared.dir",
    }
}

fn classify_session_path<'a>(mut parts: impl Iterator<Item = &'a str>) -> &'static str {
    let Some(session) = parts.next() else {
        return "ctx.session.dir";
    };
    if !is_object_name(session) {
        return "ctx.unknown";
    }

    match parts.next() {
        None => "ctx.session.dir",
        Some("messages.jsonl") if parts.next().is_none() => "ctx.session.messages",
        Some("events.jsonl") if parts.next().is_none() => "ctx.session.events",
        Some(_) => "ctx.ordinary",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AGENT_CONTROL_FILES, AgentControlIssue, AgentControlKind, AgentExecutableSocketRuntime,
        AgentRuntimeViewError, AgentUnixIdentity, CTX_ROOT, ChildAgentAuthority,
        ChildAgentControls, ChildAgentDenial, ChildAgentRequest, ChildContextRecordError,
        ChildContextStatus, ChildLifecycle, ContextJsonlIssue, ContextJsonlKind,
        ContextPackBuildError, ContextPackIssue, ContextPackSourceError, DurableSessionLayoutError,
        EXEC_OBJECTS, EventStreamIssue, FUSE_V1_ROOT_INODE, FuseV1Error, FuseV1FileType,
        FuseV1Projection, IndexedSocketSessionRecordError, MAX_FUSE_V1_SMALL_WRITE_BYTES,
        MAX_OBJECT_NAME_LEN, MAX_SOCKET_FRAME_BYTES, MODEL_CONTROL_FILES, MessageStreamIssue,
        ModelCapabilityIssue, MountEntry, MountError, MountMode, MountOption, MountTable,
        ObjectBootstrapError, ObjectClass, ObjectLayoutIssue, OwnedChildCancellationError,
        PeerCredentials, PolicyError, PolicyObjectClass, PolicyPermission, PolicyRule, PolicyV0,
        ReferenceTreeError, SESSION_REQUIRED_FILES, SHARED_QUEUE_REQUIRED_DIRS, SessionAccess,
        SessionAccessAuthority, SessionAccessDenial, SessionControlIssue, SessionControlKind,
        SessionIndexIssue, SessionIndexKind, SessionIndexUpdateError, SessionLayoutIssue,
        SharedAccess, SharedAccessAuthority, SharedAccessDenial, SharedQueueLayoutIssue,
        SharedQueueOutcome, SharedQueueRecoverError, SocketPeerPolicy, SocketRequest,
        SocketRequestError, SocketRuntimeError, SocketSessionRecordError, SocketSessionScope,
        TOOL_CONTROL_FILES, ToolExecutionAuthority, ToolExecutionDenial, ToolExecutionPrincipal,
        ToolHit, ToolPath, ToolPathError, ToolSchemaIssue, authorize_child_agent,
        authorize_session_access, authorize_shared_access, authorize_tool_execution,
        claim_next_shared_queue_job, classify_abi_path, derive_agent_runtime_view,
        ensure_durable_session_layout, ensure_v1_reference_tree, finish_shared_queue_job,
        handle_socket_request_frame, inspect_agent_control, inspect_context_jsonl,
        inspect_context_pack_json, inspect_event_stream_jsonl, inspect_message_stream_jsonl,
        inspect_model_capabilities, inspect_object_layout, inspect_session_control,
        inspect_session_index, inspect_session_layout, inspect_shared_queue_layout,
        inspect_tool_schema_json, install_executable_object_wrapper, is_object_name, is_root_entry,
        owned_child_cancellation_events, parse_socket_request_frame, peer_credentials,
        rebuild_context_pack, record_assistant_response_to_session,
        record_child_handoff_to_parent_context, record_child_result_to_parent_context,
        record_indexed_socket_send_to_session, record_owned_child_cancellation,
        record_socket_request_to_session, record_tool_execution_denial_to_session,
        record_tool_execution_result_to_session, recover_shared_queue_job,
        serve_agent_executable_socket_stream_once, serve_unix_socket_listener_once,
        serve_unix_socket_stream_once, session_index_key_for_cwd, socket_runtime_error_response,
        update_session_index, validate_context_pack_source,
    };
    use std::fs;
    use std::io::{Read, Write};
    use std::net::Shutdown;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn root_is_ctx() {
        assert_eq!(CTX_ROOT, "/ctx");
    }

    #[test]
    fn root_keeps_only_short_agent_os_entries() {
        assert!(is_root_entry("model"));
        assert!(is_root_entry("agent"));
        assert!(is_root_entry("tool"));
        assert!(!is_root_entry("provider"));
        assert!(!is_root_entry("format"));
        assert!(!is_root_entry("db"));
        assert!(!is_root_entry("vector"));
        assert!(!is_root_entry("mcp"));
        assert!(!is_root_entry("cluster"));
        assert!(!is_root_entry("audit"));
        assert!(!is_root_entry("control"));
        assert!(!is_root_entry("AGENTS.rc"));
    }

    #[test]
    fn executable_objects_are_model_agent_tool() {
        assert_eq!(EXEC_OBJECTS, ["model", "agent", "tool"]);
        assert_eq!(ObjectClass::parse("model"), Some(ObjectClass::Model));
        assert_eq!(ObjectClass::parse("agent"), Some(ObjectClass::Agent));
        assert_eq!(ObjectClass::parse("tool"), Some(ObjectClass::Tool));
        assert_eq!(ObjectClass::parse("provider"), None);
    }

    #[test]
    fn object_names_are_small_ascii_path_components() {
        assert!(is_object_name("qwen"));
        assert!(is_object_name("fs.read"));
        assert!(is_object_name("mcp.github.search_issues"));
        assert!(is_object_name("agent_1+dev-2"));
        assert!(is_object_name(&"a".repeat(MAX_OBJECT_NAME_LEN)));

        assert!(!is_object_name(""));
        assert!(!is_object_name("."));
        assert!(!is_object_name(".."));
        assert!(!is_object_name("-bad"));
        assert!(!is_object_name("_bad"));
        assert!(!is_object_name("bad/name"));
        assert!(!is_object_name("bad\nname"));
        assert!(!is_object_name("qwen.sock"));
        assert!(!is_object_name("qwen.d"));
        assert!(!is_object_name("中文"));
        assert!(!is_object_name(&"a".repeat(MAX_OBJECT_NAME_LEN + 1)));
    }

    #[test]
    fn abi_paths_classify_by_stable_shape() {
        assert_eq!(classify_abi_path("model/qwen"), "ctx.model.exec");
        assert_eq!(classify_abi_path("model/qwen.sock"), "ctx.model.socket");
        assert_eq!(classify_abi_path("model/qwen.d/id"), "ctx.model.control");
        assert_eq!(classify_abi_path("agent/coder"), "ctx.agent.exec");
        assert_eq!(classify_abi_path("agent/coder.sock"), "ctx.agent.socket");
        assert_eq!(
            classify_abi_path("agent/coder.d/policy"),
            "ctx.agent.control"
        );
        assert_eq!(classify_abi_path("tool/fs.read"), "ctx.tool.exec");
        assert_eq!(
            classify_abi_path("tool/fs.read.d/schema"),
            "ctx.tool.control"
        );
        assert_eq!(classify_abi_path("home/1000"), "ctx.home.dir");
        assert_eq!(
            classify_abi_path("home/1000/agent/coder/session/default"),
            "ctx.session.dir"
        );
        assert_eq!(
            classify_abi_path("home/1000/agent/coder/session/default/messages.jsonl"),
            "ctx.session.messages"
        );
        assert_eq!(
            classify_abi_path("home/1000/agent/coder/session/default/events.jsonl"),
            "ctx.session.events"
        );
        assert_eq!(
            classify_abi_path("home/1000/model/qwen.d/session/default"),
            "ctx.session.dir"
        );
        assert_eq!(
            classify_abi_path("shared/im-qq-dev/agent/bot/session/group-456/events.jsonl"),
            "ctx.session.events"
        );
        assert_eq!(
            classify_abi_path("shared/project-a/model/qwen.d/session/default/messages.jsonl"),
            "ctx.session.messages"
        );
        assert_eq!(classify_abi_path("shared/project-a"), "ctx.shared.dir");
        assert_eq!(
            classify_abi_path("shared/project-a/tool/project.test"),
            "ctx.shared.tool.exec"
        );
        assert_eq!(
            classify_abi_path("shared/project-a/tool/project.test.d/schema"),
            "ctx.shared.tool.control"
        );
        assert_eq!(
            classify_abi_path("shared/project-a/queue"),
            "ctx.shared.queue"
        );
        assert_eq!(
            classify_abi_path("shared/project-a/queue/pending"),
            "ctx.shared.queue"
        );
        assert_eq!(
            classify_abi_path("shared/project-a/result"),
            "ctx.shared.result"
        );
    }

    #[test]
    fn abi_path_classifier_rejects_forbidden_root_and_bad_names() {
        assert_eq!(classify_abi_path("provider/openai"), "ctx.unknown");
        assert_eq!(classify_abi_path("mcp/github"), "ctx.unknown");
        assert_eq!(classify_abi_path("skill/local"), "ctx.unknown");
        assert_eq!(classify_abi_path("cluster/default"), "ctx.unknown");
        assert_eq!(classify_abi_path("model/qwen.sock.d/id"), "ctx.unknown");
        assert_eq!(classify_abi_path("tool/-bad"), "ctx.unknown");
        assert_eq!(classify_abi_path("agent/coder/extra"), "ctx.unknown");
    }

    #[test]
    fn reference_tree_bootstrap_materializes_documented_v1_shape() {
        let root = unique_test_dir("reference-tree");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

        let bootstrapped = ensure_v1_reference_tree(&root);
        assert!(bootstrapped.is_ok());
        let Ok(bootstrapped) = bootstrapped else {
            return;
        };
        assert_eq!(bootstrapped.root(), root.as_path());

        let status = fs::read_to_string(root.join("status"));
        assert!(matches!(status, Ok(ref content) if content == "ready\n"));
        assert!(root.join("bin").join("ctx").is_file());
        let agent_socket_mode = fs::metadata(root.join("agent").join("coder.sock"))
            .map(|metadata| metadata.permissions().mode() & 0o777);
        assert!(matches!(agent_socket_mode, Ok(0o777)));
        assert!(!root.join("mcp").exists());
        assert!(!root.join("skill").exists());
        assert!(!root.join("memory").exists());

        assert!(inspect_object_layout(&root, ObjectClass::Model, "qwen").is_ok());
        assert!(inspect_object_layout(&root, ObjectClass::Agent, "coder").is_ok());
        assert!(inspect_object_layout(&root, ObjectClass::Agent, "reviewer").is_ok());
        for tool in [
            "fs.read",
            "fs.write",
            "shell.exec",
            "mcp.github.search_issues",
            "agent.create",
            "agent.start",
            "agent.stop",
        ] {
            assert!(inspect_object_layout(&root, ObjectClass::Tool, tool).is_ok());
        }
        let origin = fs::read_to_string(
            root.join("tool")
                .join("mcp.github.search_issues.d")
                .join("origin"),
        );
        assert!(matches!(origin, Ok(ref content) if content == "mcp:github\n"));

        let private_session = root
            .join("home")
            .join("1000")
            .join("agent")
            .join("coder")
            .join("session")
            .join("default");
        assert!(inspect_session_layout(&private_session).is_ok());
        assert!(
            private_session
                .join("context")
                .join("swap")
                .join("chunk")
                .is_dir()
        );
        assert!(
            private_session
                .join("context")
                .join("dedup")
                .join("blob")
                .is_dir()
        );
        assert!(
            private_session
                .join("context")
                .join("child")
                .join("rev-123")
                .join("artifact")
                .is_dir()
        );
        let child_agent = fs::read_to_string(
            private_session
                .join("context")
                .join("child")
                .join("rev-123")
                .join("agent"),
        );
        assert!(matches!(child_agent, Ok(ref content) if content == "reviewer\n"));
        let cwd_key = session_index_key_for_cwd("/work");
        assert!(cwd_key.is_some());
        let Some(cwd_key) = cwd_key else { return };
        assert!(
            root.join("home")
                .join("1000")
                .join("agent")
                .join("coder")
                .join("session")
                .join("index")
                .join("by-cwd")
                .join(cwd_key)
                .is_file()
        );

        let tool_link = fs::read_link(root.join("home").join("1000").join("tool").join("fs.read"));
        assert!(matches!(tool_link, Ok(ref target) if target == Path::new("/ctx/tool/fs.read")));
        let model_link = fs::read_link(root.join("home").join("1000").join("model").join("coder"));
        assert!(matches!(model_link, Ok(ref target) if target == Path::new("/ctx/model/qwen")));

        let shared = root.join("shared").join("project-a");
        assert!(shared.join("data").is_dir());
        assert!(shared.join("result").is_dir());
        assert!(inspect_shared_queue_layout(&shared.join("queue")).is_ok());
        assert!(shared.join("tool").join("project.test").is_file());
        for file in ["schema", "policy", "status", "log"] {
            assert!(
                shared
                    .join("tool")
                    .join("project.test.d")
                    .join(file)
                    .is_file()
            );
        }
        assert!(
            inspect_session_layout(
                &shared
                    .join("agent")
                    .join("coder")
                    .join("session")
                    .join("design-review")
            )
            .is_ok()
        );

        assert_eq!(ensure_v1_reference_tree(&root), Ok(bootstrapped));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn reference_tree_model_exec_emits_one_shot_jsonl() {
        let root = unique_test_dir("reference-tree-model-exec");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(ensure_v1_reference_tree(&root).is_ok());

        let output = Command::new(root.join("model").join("qwen"))
            .arg("hello")
            .output();
        assert!(output.is_ok());
        let Ok(output) = output else { return };
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout);
        assert!(stdout.is_ok());
        let Ok(stdout) = stdout else { return };
        assert!(stdout.contains(r#"{"type":"start","run":"r1","model":"qwen"}"#));
        assert!(stdout.contains(r#"{"type":"delta","run":"r1","text":"hello"}"#));
        assert!(stdout.contains(r#"{"type":"done","run":"r1","status":"ok"}"#));
        assert!(inspect_event_stream_jsonl(&stdout).is_ok());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn reference_tree_agent_exec_emits_one_shot_jsonl() {
        let root = unique_test_dir("reference-tree-agent-exec");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(ensure_v1_reference_tree(&root).is_ok());

        let output = Command::new(root.join("agent").join("coder"))
            .arg("fix tests")
            .output();
        assert!(output.is_ok());
        let Ok(output) = output else { return };
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout);
        assert!(stdout.is_ok());
        let Ok(stdout) = stdout else { return };
        assert!(stdout.contains(r#"{"type":"start","run":"r1","agent":"coder"}"#));
        assert!(stdout.contains(
            r#"{"type":"message","run":"r1","role":"assistant","content":[{"type":"text","text":"fix tests"}]}"#
        ));
        assert!(stdout.contains(r#"{"type":"done","run":"r1","status":"ok"}"#));
        assert!(inspect_event_stream_jsonl(&stdout).is_ok());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn reference_tree_standard_tools_emit_jsonl() {
        let root = unique_test_dir("reference-tree-tool-exec");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(ensure_v1_reference_tree(&root).is_ok());

        let data = root.join("shared").join("project-a").join("data");
        let read_target = data.join("readme.txt");
        write_text_file(&read_target, "visible");
        let read_arg = format!(r#"{{"path":"{}"}}"#, read_target.display());
        let read = Command::new(root.join("tool").join("fs.read"))
            .arg(read_arg)
            .output();
        assert!(read.is_ok());
        let Ok(read) = read else { return };
        assert!(read.status.success());
        let read_stdout = String::from_utf8(read.stdout);
        assert!(read_stdout.is_ok());
        let Ok(read_stdout) = read_stdout else {
            return;
        };
        assert!(read_stdout.contains(r#"{"type":"start","run":"r1","tool":"fs.read"}"#));
        assert!(read_stdout.contains(r#""text":"visible""#));
        assert!(inspect_event_stream_jsonl(&read_stdout).is_ok());

        let write_target = data.join("written.txt");
        let write_arg = format!(
            r#"{{"path":"{}","content":"stored"}}"#,
            write_target.display()
        );
        let write = Command::new(root.join("tool").join("fs.write"))
            .arg(write_arg)
            .output();
        assert!(write.is_ok());
        let Ok(write) = write else { return };
        assert!(write.status.success());
        let written = fs::read_to_string(&write_target);
        assert!(matches!(written, Ok(ref content) if content == "stored"));
        let write_stdout = String::from_utf8(write.stdout);
        assert!(write_stdout.is_ok());
        let Ok(write_stdout) = write_stdout else {
            return;
        };
        assert!(write_stdout.contains(r#"{"type":"start","run":"r1","tool":"fs.write"}"#));
        assert!(inspect_event_stream_jsonl(&write_stdout).is_ok());

        let shell = Command::new(root.join("tool").join("shell.exec"))
            .arg(r#"{"cmd":"printf shell-ok"}"#)
            .output();
        assert!(shell.is_ok());
        let Ok(shell) = shell else { return };
        assert!(shell.status.success());
        let shell_stdout = String::from_utf8(shell.stdout);
        assert!(shell_stdout.is_ok());
        let Ok(shell_stdout) = shell_stdout else {
            return;
        };
        assert!(shell_stdout.contains(r#"{"type":"start","run":"r1","tool":"shell.exec"}"#));
        assert!(shell_stdout.contains(r#""text":"shell-ok""#));
        assert!(inspect_event_stream_jsonl(&shell_stdout).is_ok());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn fuse_v1_projection_exposes_reference_tree_ops() {
        let root = unique_test_dir("fuse-v1-projection");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(ensure_v1_reference_tree(&root).is_ok());
        let projection = FuseV1Projection::new(&root);

        let root_node = projection.root_node();
        assert!(root_node.is_ok());
        let Ok(root_node) = root_node else { return };
        assert_eq!(root_node.inode(), FUSE_V1_ROOT_INODE);
        assert_eq!(root_node.abi_path(), "");
        assert_eq!(root_node.attr().file_type(), FuseV1FileType::Directory);

        let root_attr = projection.getattr_node(&root_node);
        assert!(matches!(
            root_attr,
            Ok(ref attr)
                if attr.abi_path().is_empty()
                    && attr.file_type() == FuseV1FileType::Directory
        ));

        let entries = projection.readdir_node(&root_node);
        assert!(entries.is_ok());
        let Ok(entries) = entries else { return };
        let names = entries
            .iter()
            .map(super::FuseV1DirEntry::name)
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            ["agent", "bin", "home", "model", "shared", "status", "tool"]
        );

        let model_node = projection.lookup(&root_node, "model");
        assert!(matches!(
            model_node,
            Ok(ref node)
                if node.abi_path() == "model"
                    && node.attr().file_type() == FuseV1FileType::Directory
        ));
        let Ok(model_node) = model_node else { return };
        let qwen_node = projection.lookup(&model_node, "qwen");
        assert!(matches!(
            qwen_node,
            Ok(ref node)
                if node.abi_path() == "model/qwen"
                    && node.inode() != FUSE_V1_ROOT_INODE
                    && node.attr().file_type() == FuseV1FileType::Regular
        ));
        let qwen_again = projection.node_for_path("model/qwen");
        assert!(
            matches!((qwen_node, qwen_again), (Ok(ref left), Ok(ref right)) if left.inode() == right.inode())
        );
        assert_eq!(
            projection.lookup(&root_node, "../escape"),
            Err(FuseV1Error::InvalidPath)
        );
        assert_eq!(
            projection.lookup(&root_node, "missing"),
            Err(FuseV1Error::NotFound)
        );

        let socket_attr = projection.getattr("model/qwen.sock");
        assert!(matches!(
            socket_attr,
            Ok(ref attr)
                if attr.file_type() == FuseV1FileType::Socket && attr.mode() & 0o777 == 0o777
        ));
        let symlink_attr = projection.getattr("home/1000/tool/fs.read");
        assert!(matches!(
            symlink_attr,
            Ok(ref attr) if attr.file_type() == FuseV1FileType::Symlink
        ));

        assert_eq!(
            projection.read_to_string("status"),
            Ok("ready\n".to_owned())
        );
        assert_eq!(projection.read_at("status", 1, 3), Ok(b"ead".to_vec()));
        assert_eq!(projection.read_at("status", 128, 8), Ok(Vec::new()));
        assert!(
            projection
                .write_control_file("agent/coder.d/cwd", "/work/project\n")
                .is_ok()
        );
        assert_eq!(
            projection.read_to_string("agent/coder.d/cwd"),
            Ok("/work/project\n".to_owned())
        );

        assert_eq!(
            projection.write_control_file("status", "busy\n"),
            Err(FuseV1Error::NotControlFile)
        );
        assert!(
            projection
                .write_control_file_at("agent/coder.d/status", 0, b"busy\n")
                .is_ok()
        );
        assert_eq!(
            projection.read_to_string("agent/coder.d/status"),
            Ok("busy\n".to_owned())
        );
        assert_eq!(
            projection.write_control_file_at("agent/coder.d/status", 1, b"idle\n"),
            Err(FuseV1Error::InvalidOffset)
        );
        assert_eq!(
            projection.write_control_file_at("agent/coder.d/status", 0, &[0xff]),
            Err(FuseV1Error::InvalidContent)
        );
        assert_eq!(
            projection.write_control_file("../escape", "no\n"),
            Err(FuseV1Error::InvalidPath)
        );
        assert_eq!(
            projection.write_control_file(
                "agent/coder.d/cwd",
                &"x".repeat(MAX_FUSE_V1_SMALL_WRITE_BYTES + 1)
            ),
            Err(FuseV1Error::TooLarge)
        );
        assert_eq!(FuseV1Error::TooLarge.errno(), "EMSGSIZE");
        assert_eq!(FuseV1Error::InvalidOffset.errno(), "EINVAL");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn reference_tree_bootstrap_rejects_conflicting_symlink_and_socket_paths() {
        let root = unique_test_dir("reference-tree-conflict");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        write_text_file(
            &root.join("home").join("1000").join("tool").join("fs.read"),
            "not link\n",
        );
        assert_eq!(
            ensure_v1_reference_tree(&root),
            Err(ReferenceTreeError::CannotLink)
        );

        assert!(fs::remove_dir_all(&root).is_ok());
        write_text_file(&root.join("model").join("qwen.sock"), "not socket\n");
        assert_eq!(
            ensure_v1_reference_tree(&root),
            Err(ReferenceTreeError::CannotSocket)
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn object_layout_accepts_model_agent_and_tool_triples() {
        let root = unique_test_dir("object-layout-ok");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_object_layout(&root, ObjectClass::Model, "qwen", "socket");
        create_complete_object_layout(&root, ObjectClass::Agent, "coder", "");
        create_complete_object_layout(&root, ObjectClass::Tool, "fs.read", "");
        let _model_socket = bind_socket(&root.join("model").join("qwen.sock"));
        let _agent_socket = bind_socket(&root.join("agent").join("coder.sock"));

        let model = inspect_object_layout(&root, ObjectClass::Model, "qwen");
        let agent = inspect_object_layout(&root, ObjectClass::Agent, "coder");
        let tool = inspect_object_layout(&root, ObjectClass::Tool, "fs.read");
        assert!(model.is_ok());
        assert!(agent.is_ok());
        assert!(tool.is_ok());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn executable_object_bootstrap_installs_model_and_tool_wrappers() {
        let root = unique_test_dir("object-bootstrap");
        let target = root.join("runtime").join("echo-jsonl");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        write_fixture_file(&target, 0o755);

        let model = install_executable_object_wrapper(
            &root,
            ObjectClass::Model,
            "qwen",
            &target.display().to_string(),
            &[
                ("cap", "chat\nstream\ntool_call_syntax"),
                ("session", "none"),
                ("id", "local/qwen"),
            ],
        );
        assert!(model.is_ok());
        let Ok(model) = model else { return };
        let tool = install_executable_object_wrapper(
            &root,
            ObjectClass::Tool,
            "fs.read",
            &target.display().to_string(),
            &[
                ("description", "Read a visible file"),
                ("schema", "{\"type\":\"object\",\"properties\":{}}"),
                ("policy", "allow coder_t tool:fs.read execute"),
            ],
        );
        assert!(tool.is_ok());
        let Ok(tool) = tool else { return };

        assert_eq!(model.executable(), root.join("model").join("qwen"));
        assert_eq!(tool.control_dir(), root.join("tool").join("fs.read.d"));
        assert!(inspect_object_layout(&root, ObjectClass::Model, "qwen").is_ok());
        assert!(inspect_object_layout(&root, ObjectClass::Tool, "fs.read").is_ok());

        let wrapper = fs::read_to_string(root.join("tool").join("fs.read"));
        assert!(wrapper.is_ok());
        let Ok(wrapper) = wrapper else { return };
        assert!(wrapper.starts_with("#!/bin/sh\n"));
        assert!(wrapper.contains("exec '"));
        let permissions = fs::metadata(root.join("tool").join("fs.read"))
            .map(|metadata| metadata.permissions().mode());
        assert!(permissions.is_ok());
        let Ok(permissions) = permissions else {
            return;
        };
        assert_ne!(permissions & 0o111, 0);

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn executable_object_bootstrap_validates_controls_and_agent_socket_boundary() {
        let root = unique_test_dir("object-bootstrap-bad");
        let target = root.join("runtime").join("agent");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        write_fixture_file(&target, 0o755);

        assert_eq!(
            install_executable_object_wrapper(
                &root,
                ObjectClass::Tool,
                "bad/name",
                &target.display().to_string(),
                &[],
            ),
            Err(ObjectBootstrapError::InvalidObjectName)
        );
        assert_eq!(
            install_executable_object_wrapper(&root, ObjectClass::Tool, "fs.read", "bad\ncmd", &[]),
            Err(ObjectBootstrapError::InvalidWrapperTarget)
        );
        assert_eq!(
            install_executable_object_wrapper(
                &root,
                ObjectClass::Tool,
                "fs.read",
                &target.display().to_string(),
                &[("authority", "root")],
            ),
            Err(ObjectBootstrapError::InvalidControlFile)
        );
        assert_eq!(
            install_executable_object_wrapper(
                &root,
                ObjectClass::Tool,
                "fs.read",
                &target.display().to_string(),
                &[("schema", "{\"authority\":\"root\"}")],
            ),
            Err(ObjectBootstrapError::InvalidControlValue)
        );

        let agent = install_executable_object_wrapper(
            &root,
            ObjectClass::Agent,
            "coder",
            &target.display().to_string(),
            &[("uid", "1000"), ("gid", "1000"), ("owner", "1000")],
        );
        assert!(agent.is_ok());
        let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
        assert!(!report.is_ok());
        assert!(report.issues().contains(&ObjectLayoutIssue::MissingSocket(
            "agent/coder.sock".to_owned()
        )));
        let _agent_socket = bind_socket(&root.join("agent").join("coder.sock"));
        assert!(inspect_object_layout(&root, ObjectClass::Agent, "coder").is_ok());
        assert_eq!(ObjectBootstrapError::InvalidControlValue.errno(), "EINVAL");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn object_layout_reports_missing_parts() {
        let root = unique_test_dir("object-layout-bad");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(root.join("agent")).is_ok());
        write_text_file(&root.join("agent").join("coder"), "#!/bin/sh\n");

        let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
        assert!(!report.is_ok());
        assert!(
            report
                .issues()
                .contains(&ObjectLayoutIssue::NotExecutable("agent/coder".to_owned()))
        );
        assert!(
            report
                .issues()
                .contains(&ObjectLayoutIssue::MissingControlDirectory(
                    "agent/coder.d".to_owned()
                ))
        );
        assert!(report.issues().contains(&ObjectLayoutIssue::MissingSocket(
            "agent/coder.sock".to_owned()
        )));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn model_session_control_decides_socket_requirement() {
        let root = unique_test_dir("object-layout-model-session");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_object_layout(&root, ObjectClass::Model, "qwen", "none");

        let no_socket = inspect_object_layout(&root, ObjectClass::Model, "qwen");
        assert!(no_socket.is_ok());

        write_text_file(
            &root.join("model").join("qwen.d").join("session"),
            "socket\n",
        );
        let missing_socket = inspect_object_layout(&root, ObjectClass::Model, "qwen");
        assert!(
            missing_socket
                .issues()
                .contains(&ObjectLayoutIssue::MissingSocket(
                    "model/qwen.sock".to_owned()
                ))
        );

        write_text_file(
            &root.join("model").join("qwen.d").join("session"),
            "native_thread\n",
        );
        let invalid = inspect_object_layout(&root, ObjectClass::Model, "qwen");
        assert!(
            invalid
                .issues()
                .contains(&ObjectLayoutIssue::InvalidControlValue {
                    path: "model/qwen.d/session".to_owned(),
                    value: "native_thread".to_owned()
                })
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn model_capabilities_accept_only_stable_words() {
        let valid = inspect_model_capabilities("chat\nstream\ntool_call_syntax\n\n");
        assert!(valid.is_ok());

        let invalid = inspect_model_capabilities("openai_responses\nnative_thread\nvendor_magic\n");
        assert_eq!(
            invalid.issues(),
            &[
                ModelCapabilityIssue::ProviderPrivate {
                    line: 1,
                    capability: "openai_responses".to_owned()
                },
                ModelCapabilityIssue::ProviderPrivate {
                    line: 2,
                    capability: "native_thread".to_owned()
                },
                ModelCapabilityIssue::Unknown {
                    line: 3,
                    capability: "vendor_magic".to_owned()
                }
            ]
        );
    }

    #[test]
    fn model_object_layout_rejects_provider_private_capabilities() {
        let root = unique_test_dir("object-layout-model-cap");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_object_layout(&root, ObjectClass::Model, "qwen", "none");
        write_text_file(
            &root.join("model").join("qwen.d").join("cap"),
            "chat\nnative_thread\n",
        );

        let report = inspect_object_layout(&root, ObjectClass::Model, "qwen");
        assert!(
            report
                .issues()
                .contains(&ObjectLayoutIssue::InvalidControlValue {
                    path: "model/qwen.d/cap".to_owned(),
                    value: "native_thread".to_owned()
                })
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_schema_accepts_json_schema_shape_without_authority() {
        let report = inspect_tool_schema_json(
            r#"{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}"#,
        );
        assert!(report.is_ok());
        assert!(report.issues().is_empty());
    }

    #[test]
    fn tool_schema_rejects_invalid_json_and_authority_fields() {
        assert_eq!(
            inspect_tool_schema_json("not-json").issues(),
            &[ToolSchemaIssue::InvalidJson]
        );
        assert_eq!(
            inspect_tool_schema_json("[]").issues(),
            &[ToolSchemaIssue::NotObject]
        );
        assert_eq!(
            inspect_tool_schema_json(r#"{"policy":"allow all","permissions":["tool:*"]}"#).issues(),
            &[
                ToolSchemaIssue::AuthorityField("permissions".to_owned()),
                ToolSchemaIssue::AuthorityField("policy".to_owned())
            ]
        );
    }

    #[test]
    fn tool_object_layout_rejects_authority_shaped_schema() {
        let root = unique_test_dir("object-layout-tool-schema");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_object_layout(&root, ObjectClass::Tool, "fs.read", "none");
        write_text_file(
            &root.join("tool").join("fs.read.d").join("schema"),
            "{\"policy\":\"allow all\"}\n",
        );

        let report = inspect_object_layout(&root, ObjectClass::Tool, "fs.read");
        assert!(
            report
                .issues()
                .contains(&ObjectLayoutIssue::InvalidControlValue {
                    path: "tool/fs.read.d/schema".to_owned(),
                    value: "policy".to_owned()
                })
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn agent_controls_accept_fixed_v1_values() {
        assert!(inspect_agent_control(AgentControlKind::Owner, "1000\n").is_ok());
        assert!(inspect_agent_control(AgentControlKind::Uid, "1000\n").is_ok());
        assert!(inspect_agent_control(AgentControlKind::Gid, "100\n").is_ok());
        assert!(inspect_agent_control(AgentControlKind::Groups, "10\n20\n").is_ok());
        assert!(inspect_agent_control(AgentControlKind::Groups, "").is_ok());
        assert!(inspect_agent_control(AgentControlKind::Iso, "shared\n").is_ok());
        assert!(inspect_agent_control(AgentControlKind::Iso, "uid\n").is_ok());
        assert!(inspect_agent_control(AgentControlKind::Life, "owned\n").is_ok());
        assert!(inspect_agent_control(AgentControlKind::Parent, "\n").is_ok());
        assert!(
            inspect_agent_control(
                AgentControlKind::Parent,
                "agent:coder session:default run:r1\n"
            )
            .is_ok()
        );
        assert!(inspect_agent_control(AgentControlKind::Status, "idle\n").is_ok());
        assert!(inspect_agent_control(AgentControlKind::Pid, "\n").is_ok());
        assert!(inspect_agent_control(AgentControlKind::Pid, "1234\n").is_ok());
    }

    #[test]
    fn agent_controls_reject_invalid_identity_lifecycle_and_parent() {
        assert_eq!(
            inspect_agent_control(AgentControlKind::Uid, "not-a-uid\n").issues(),
            &[AgentControlIssue::InvalidNumber {
                line: 1,
                value: "not-a-uid".to_owned()
            }]
        );
        assert_eq!(
            inspect_agent_control(AgentControlKind::Groups, "10\nbad\n").issues(),
            &[AgentControlIssue::InvalidNumber {
                line: 2,
                value: "bad".to_owned()
            }]
        );
        assert_eq!(
            inspect_agent_control(AgentControlKind::Life, "detached\n").issues(),
            &[AgentControlIssue::InvalidValue {
                line: 1,
                value: "detached".to_owned()
            }]
        );
        assert_eq!(
            inspect_agent_control(AgentControlKind::Parent, "coder session:default\n").issues(),
            &[AgentControlIssue::InvalidValue {
                line: 1,
                value: "coder session:default".to_owned()
            }]
        );
        assert_eq!(
            inspect_agent_control(AgentControlKind::Status, "running\nextra\n").issues(),
            &[
                AgentControlIssue::InvalidValue {
                    line: 1,
                    value: "running".to_owned()
                },
                AgentControlIssue::MultipleValues { line: 2 }
            ]
        );
    }

    #[test]
    fn agent_object_layout_rejects_invalid_control_values() {
        let root = unique_test_dir("object-layout-agent-controls");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
        let control = root.join("agent").join("coder.d");
        write_text_file(&control.join("iso"), "container\n");
        write_text_file(&control.join("uid"), "bad\n");

        let report = inspect_object_layout(&root, ObjectClass::Agent, "coder");
        assert!(
            report
                .issues()
                .contains(&ObjectLayoutIssue::InvalidControlValue {
                    path: "agent/coder.d/iso".to_owned(),
                    value: "container".to_owned()
                })
        );
        assert!(
            report
                .issues()
                .contains(&ObjectLayoutIssue::InvalidControlValue {
                    path: "agent/coder.d/uid".to_owned(),
                    value: "bad".to_owned()
                })
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn agent_runtime_view_derives_identity_environment_policy_and_view() {
        let root = unique_test_dir("agent-runtime-view");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
        let control = root.join("agent").join("coder.d");
        write_text_file(
            &control.join("env"),
            "CTX_ROOT=/ignored\nHOME=/ignored\nRUST_LOG=info\n",
        );

        let view = derive_agent_runtime_view(&root, "coder");
        assert!(view.is_ok());
        let Ok(view) = view else { return };

        assert_eq!(view.agent_name(), "coder");
        assert_eq!(view.control_dir(), control.as_path());
        assert_eq!(view.ctx_root(), root.as_path());
        assert_eq!(view.ctx_home(), root.join("home").join("1000").as_path());
        assert_eq!(
            view.home(),
            root.join("home")
                .join("1000")
                .join("agent")
                .join("coder")
                .as_path()
        );
        assert_eq!(view.owner(), 1000);
        assert_eq!(view.identity().uid(), 1000);
        assert_eq!(view.identity().gid(), 100);
        assert_eq!(view.identity().groups(), &[10, 20]);
        assert_eq!(view.label(), "user_u:agent_r:coder_t:s0");
        assert_eq!(view.policy_subject(), "coder_t");
        assert_eq!(view.iso(), "shared");
        assert_eq!(view.parent(), None);
        assert_eq!(view.lifecycle(), ChildLifecycle::Owned);
        assert_eq!(view.root(), Path::new("/ctx/home/1000/agent/coder/root"));
        assert_eq!(view.cwd(), Path::new("/work"));
        assert_eq!(view.model(), "qwen");
        assert_eq!(
            view.tool_path().dirs(),
            [
                PathBuf::from("/ctx/tool"),
                PathBuf::from("/ctx/home/1000/tool")
            ]
        );
        assert_eq!(view.mount_table().entries().len(), 1);
        assert!(view.policy().allows(
            "coder_t",
            PolicyObjectClass::Model,
            "qwen",
            PolicyPermission::Use,
        ));
        assert_eq!(
            env_value(view.env(), "CTX_ROOT").map(str::to_owned),
            Some(root.display().to_string())
        );
        assert_eq!(
            env_value(view.env(), "CTX_HOME").map(str::to_owned),
            Some(root.join("home").join("1000").display().to_string())
        );
        assert_eq!(
            env_value(view.env(), "HOME").map(str::to_owned),
            Some(
                root.join("home")
                    .join("1000")
                    .join("agent")
                    .join("coder")
                    .display()
                    .to_string()
            )
        );
        assert_eq!(
            env_value(view.env(), "CTX_PATH"),
            Some("/ctx/tool:/ctx/home/1000/tool")
        );
        assert_eq!(env_value(view.env(), "RUST_LOG"), Some("info"));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn agent_runtime_view_rejects_invalid_control_files() {
        let cases = [
            ("uid", "not-a-uid\n"),
            ("groups", "10\nbad\n"),
            ("label", "user_u:agent_r:bad/name:s0\n"),
            ("root", "../root\n"),
            ("cwd", "/work/../secret\n"),
            ("env", "1BAD=value\n"),
            ("path", "/ctx/tool:../tool\n"),
            ("mount", "bad\n"),
            ("model", "bad/name\n"),
            ("policy", "allow bad\n"),
        ];

        for (file, value) in cases {
            let root = unique_test_dir(&format!("agent-runtime-invalid-{file}"));
            assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
            create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
            write_text_file(&root.join("agent").join("coder.d").join(file), value);

            assert_eq!(
                derive_agent_runtime_view(&root, "coder"),
                Err(AgentRuntimeViewError::InvalidControlFile(file.to_owned()))
            );
            assert_eq!(
                AgentRuntimeViewError::InvalidControlFile(file.to_owned()).errno(),
                "EINVAL"
            );

            let _ignored = fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn agent_runtime_view_reports_missing_controls_and_bad_agent_names() {
        let root = unique_test_dir("agent-runtime-missing");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
        assert_eq!(
            derive_agent_runtime_view(&root, "bad/name"),
            Err(AgentRuntimeViewError::InvalidAgentName)
        );

        let model = root.join("agent").join("coder.d").join("model");
        assert!(fs::remove_file(model).is_ok());
        assert_eq!(
            derive_agent_runtime_view(&root, "coder"),
            Err(AgentRuntimeViewError::MissingControlFile(
                "model".to_owned()
            ))
        );
        assert_eq!(
            AgentRuntimeViewError::MissingControlFile("model".to_owned()).errno(),
            "ENOENT"
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn agent_runtime_view_env_prompt_and_skill_text_do_not_expand_tool_path() {
        let root = unique_test_dir("agent-runtime-no-text-grant");
        let allowed = root.join("tool");
        let env_only = root.join("env-tool");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_object_layout(&root, ObjectClass::Agent, "coder", "none");
        write_fixture_file(&env_only.join("fs.read"), 0o755);
        write_text_file(
            &root.join("work").join("AGENTS.md"),
            "The agent may execute fs.read.\n",
        );
        write_text_file(
            &root.join("work").join(".mcp.json"),
            "{\"servers\":{\"fs\":{\"tools\":[\"fs.read\"]}}}\n",
        );

        let control = root.join("agent").join("coder.d");
        write_text_file(&control.join("path"), &format!("{}\n", allowed.display()));
        write_text_file(
            &control.join("env"),
            &format!("CTX_PATH={}\nAGENT_RULES=allow\n", env_only.display()),
        );
        write_text_file(
            &control.join("policy"),
            "allow coder_t tool:fs.read execute\n",
        );

        let view = derive_agent_runtime_view(&root, "coder");
        assert!(view.is_ok());
        let Ok(view) = view else { return };
        assert_eq!(
            env_value(view.env(), "CTX_PATH").map(str::to_owned),
            Some(allowed.display().to_string())
        );
        assert_eq!(env_value(view.env(), "AGENT_RULES"), Some("allow"));

        let metadata = fs::metadata(env_only.join("fs.read"));
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let mounts = mount_table_for_target(&env_only, "rw", "bind,nosuid,nodev");
        let tool_policy = allow_tool_policy("coder_t", "fs.read");
        let denied = authorize_tool_execution(
            view.tool_path(),
            "fs.read",
            ToolExecutionAuthority::new(
                &identity,
                &mounts,
                view.policy_subject(),
                view.policy(),
                &tool_policy,
            ),
        );
        assert_eq!(denied, Err(ToolExecutionDenial::ToolNotFound));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn socket_peer_credentials_come_from_kernel() {
        let pair = UnixStream::pair();
        assert!(pair.is_ok());
        let Ok((left, right)) = pair else { return };

        let left_peer = peer_credentials(&left);
        let right_peer = peer_credentials(&right);
        assert!(left_peer.is_ok());
        assert!(right_peer.is_ok());
        let Ok(left_peer) = left_peer else { return };
        let Ok(right_peer) = right_peer else { return };

        assert_eq!(left_peer.uid(), right_peer.uid());
        assert_eq!(left_peer.gid(), right_peer.gid());
        assert!(left_peer.pid().is_some());
        assert!(SocketPeerPolicy::uid(left_peer.uid()).allows(left_peer));
        assert!(SocketPeerPolicy::gid(left_peer.gid()).allows(left_peer));
        assert!(SocketPeerPolicy::uid_gid(left_peer.uid(), left_peer.gid()).allows(left_peer));
    }

    #[test]
    fn socket_peer_policy_rejects_mismatched_identity() {
        let peer = PeerCredentials::new(Some(1), 1000, 100);
        assert!(SocketPeerPolicy::uid(1000).allows(peer));
        assert!(SocketPeerPolicy::gid(100).allows(peer));
        assert!(SocketPeerPolicy::uid_gid(1000, 100).allows(peer));
        assert!(!SocketPeerPolicy::uid(1001).allows(peer));
        assert!(!SocketPeerPolicy::gid(101).allows(peer));
        assert!(!SocketPeerPolicy::uid_gid(1000, 101).allows(peer));
    }

    #[test]
    fn socket_request_parser_accepts_stable_request_frames() {
        assert_eq!(
            parse_socket_request_frame(
                r#"{"op":"send","id":"msg-1","session":"default","scope":"shared","cwd":"/work","input":"hello","thread_id":"ignored"}
"#
            ),
            Ok(SocketRequest::Send {
                id: "msg-1".to_owned(),
                session: "default".to_owned(),
                scope: SocketSessionScope::Shared,
                cwd: Some("/work".to_owned()),
                input: "hello".to_owned()
            })
        );
        assert_eq!(
            parse_socket_request_frame(
                r#"{"op":"resume","session":"default","after":"event-123"}"#
            ),
            Ok(SocketRequest::Resume {
                session: "default".to_owned(),
                after: Some("event-123".to_owned())
            })
        );
        assert_eq!(
            parse_socket_request_frame(r#"{"op":"cancel","id":"run-1"}"#),
            Ok(SocketRequest::Cancel {
                id: "run-1".to_owned()
            })
        );
        assert_eq!(
            parse_socket_request_frame(r#"{"op":"ping"}"#),
            Ok(SocketRequest::Ping)
        );
    }

    #[test]
    fn socket_request_parser_defaults_session_and_scope() {
        assert_eq!(
            parse_socket_request_frame(r#"{"op":"send","id":"msg-1","input":"hello"}"#),
            Ok(SocketRequest::Send {
                id: "msg-1".to_owned(),
                session: "default".to_owned(),
                scope: SocketSessionScope::Private,
                cwd: None,
                input: "hello".to_owned()
            })
        );
        assert_eq!(
            parse_socket_request_frame(r#"{"op":"resume"}"#),
            Ok(SocketRequest::Resume {
                session: "default".to_owned(),
                after: None
            })
        );
        assert_eq!(SocketSessionScope::Temp.as_str(), "temp");
    }

    #[test]
    fn socket_request_parser_reports_stable_errno_for_bad_frames() {
        let oversized = "x".repeat(MAX_SOCKET_FRAME_BYTES + 1);
        let error = parse_socket_request_frame(&oversized);
        assert!(matches!(
            error,
            Err(SocketRequestError::FrameTooLarge { bytes }) if bytes == MAX_SOCKET_FRAME_BYTES + 1
        ));
        assert_eq!(
            error.err().as_ref().map(SocketRequestError::errno),
            Some("EMSGSIZE")
        );

        let invalid = parse_socket_request_frame("{}");
        assert_eq!(invalid, Err(SocketRequestError::MissingOp));
        assert_eq!(
            invalid.err().as_ref().map(SocketRequestError::errno),
            Some("EINVAL")
        );
    }

    #[test]
    fn socket_request_parser_rejects_invalid_ops_and_fields() {
        assert_eq!(
            parse_socket_request_frame(""),
            Err(SocketRequestError::EmptyFrame)
        );
        assert_eq!(
            parse_socket_request_frame("{\"op\":\"ping\"}\n{\"op\":\"ping\"}\n"),
            Err(SocketRequestError::MultipleFrames)
        );
        assert_eq!(
            parse_socket_request_frame("[1]"),
            Err(SocketRequestError::RequestNotObject)
        );
        assert_eq!(
            parse_socket_request_frame(r#"{"op":"native_thread"}"#),
            Err(SocketRequestError::UnknownOp("native_thread".to_owned()))
        );
        assert_eq!(
            parse_socket_request_frame(r#"{"op":"send","id":"bad/id","input":"hello"}"#),
            Err(SocketRequestError::InvalidField {
                field: "id",
                value: "bad/id".to_owned()
            })
        );
        assert_eq!(
            parse_socket_request_frame(
                r#"{"op":"send","id":"msg-1","scope":"global","input":"hello"}"#
            ),
            Err(SocketRequestError::InvalidField {
                field: "scope",
                value: "global".to_owned()
            })
        );
        assert_eq!(
            parse_socket_request_frame(
                r#"{"op":"send","id":"msg-1","cwd":"/work/../secret","input":"hello"}"#
            ),
            Err(SocketRequestError::InvalidField {
                field: "cwd",
                value: "/work/../secret".to_owned()
            })
        );
        assert_eq!(
            parse_socket_request_frame(r#"{"op":"send","id":"msg-1","input":42}"#),
            Err(SocketRequestError::MissingStringField("input"))
        );
    }

    #[test]
    fn socket_session_recorder_appends_send_to_durable_history() {
        let root = unique_test_dir("socket-session-send");
        let session = root.join("default");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&session);
        write_text_file(&session.join("messages.jsonl"), "");
        write_text_file(&session.join("events.jsonl"), "");

        let request = parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
        );
        assert!(request.is_ok());
        let Ok(request) = request else { return };
        let recorded = record_socket_request_to_session(&session, &request);
        assert!(recorded.is_ok());
        let Ok(recorded) = recorded else { return };
        assert_eq!(recorded.messages().len(), 1);
        assert_eq!(recorded.events().len(), 1);

        let messages = fs::read_to_string(session.join("messages.jsonl"));
        assert!(messages.is_ok());
        let Ok(messages) = messages else { return };
        let events = fs::read_to_string(session.join("events.jsonl"));
        assert!(events.is_ok());
        let Ok(events) = events else { return };
        assert!(inspect_message_stream_jsonl(&messages).is_ok());
        assert!(inspect_event_stream_jsonl(&events).is_ok());
        assert!(messages.contains("\"role\":\"user\""));
        assert!(messages.contains("\"content\":\"hello\""));
        assert!(events.contains("\"type\":\"start\""));
        let state = fs::read_to_string(session.join("state"));
        assert!(state.is_ok());
        let Ok(state) = state else { return };
        assert_eq!(state, "active\n");
        let cwd = fs::read_to_string(session.join("cwd"));
        assert!(cwd.is_ok());
        let Ok(cwd) = cwd else { return };
        assert_eq!(cwd, "/work/project\n");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn socket_session_recorder_cancels_without_deleting_history() {
        let root = unique_test_dir("socket-session-cancel");
        let session = root.join("default");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&session);
        write_text_file(
            &session.join("messages.jsonl"),
            "{\"role\":\"user\",\"content\":\"keep me\"}\n",
        );
        write_text_file(&session.join("events.jsonl"), "");

        let request = parse_socket_request_frame(r#"{"op":"cancel","id":"run-1"}"#);
        assert!(request.is_ok());
        let Ok(request) = request else { return };
        let recorded = record_socket_request_to_session(&session, &request);
        assert!(recorded.is_ok());
        let Ok(recorded) = recorded else { return };
        assert!(recorded.messages().is_empty());
        assert_eq!(recorded.events().len(), 1);

        let messages = fs::read_to_string(session.join("messages.jsonl"));
        assert!(messages.is_ok());
        let Ok(messages) = messages else { return };
        let events = fs::read_to_string(session.join("events.jsonl"));
        assert!(events.is_ok());
        let Ok(events) = events else { return };
        assert_eq!(messages, "{\"role\":\"user\",\"content\":\"keep me\"}\n");
        assert!(inspect_event_stream_jsonl(&events).is_ok());
        assert!(events.contains("\"status\":\"cancelled\""));
        let state = fs::read_to_string(session.join("state"));
        assert!(state.is_ok());
        let Ok(state) = state else { return };
        assert_eq!(state, "cancelled\n");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn assistant_response_recorder_updates_latest_without_replacing_history() {
        let root = unique_test_dir("assistant-response-record");
        let session = root.join("default");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&session);
        write_text_file(
            &session.join("messages.jsonl"),
            "{\"role\":\"user\",\"content\":\"hello\"}\n",
        );
        write_text_file(
            &session.join("events.jsonl"),
            "{\"type\":\"start\",\"id\":\"run-1\",\"run\":\"run-1\"}\n",
        );
        write_text_file(&session.join("latest.md"), "old\n");

        let recorded = record_assistant_response_to_session(&session, "run-1", "hello back");
        assert!(recorded.is_ok());
        let Ok(recorded) = recorded else { return };
        assert_eq!(recorded.messages().len(), 1);
        assert_eq!(recorded.events().len(), 2);

        let messages = fs::read_to_string(session.join("messages.jsonl"));
        assert!(messages.is_ok());
        let Ok(messages) = messages else { return };
        let events = fs::read_to_string(session.join("events.jsonl"));
        assert!(events.is_ok());
        let Ok(events) = events else { return };
        let latest = fs::read_to_string(session.join("latest.md"));
        assert!(latest.is_ok());
        let Ok(latest) = latest else { return };
        let state = fs::read_to_string(session.join("state"));
        assert!(state.is_ok());
        let Ok(state) = state else { return };

        assert!(inspect_message_stream_jsonl(&messages).is_ok());
        assert!(inspect_event_stream_jsonl(&events).is_ok());
        assert!(messages.contains("\"role\":\"user\""));
        assert!(messages.contains("\"role\":\"assistant\""));
        assert!(events.contains("\"type\":\"message\""));
        assert!(events.contains("\"status\":\"ok\""));
        assert_eq!(latest, "hello back\n");
        assert_eq!(state, "done\n");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_denial_recorder_makes_permission_failure_inspectable() {
        let root = unique_test_dir("tool-denial-record");
        let session = root.join("default");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&session);
        write_text_file(
            &session.join("events.jsonl"),
            "{\"type\":\"start\",\"id\":\"run-1\",\"run\":\"run-1\"}\n",
        );

        let recorded = record_tool_execution_denial_to_session(
            &session,
            "run-1",
            "fs.read",
            ToolExecutionDenial::AgentPolicy,
        );
        assert!(recorded.is_ok());
        let Ok(recorded) = recorded else { return };
        assert!(recorded.messages().is_empty());
        assert_eq!(recorded.events().len(), 2);

        let events = fs::read_to_string(session.join("events.jsonl"));
        assert!(events.is_ok());
        let Ok(events) = events else { return };
        let state = fs::read_to_string(session.join("state"));
        assert!(state.is_ok());
        let Ok(state) = state else { return };

        assert!(inspect_event_stream_jsonl(&events).is_ok());
        assert!(events.contains("\"type\":\"error\""));
        assert!(events.contains("\"tool\":\"fs.read\""));
        assert!(events.contains("\"code\":\"EACCES\""));
        assert!(events.contains("\"status\":\"error\""));
        assert_eq!(state, "error\n");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_denial_recorder_rejects_invalid_tool_names() {
        let root = unique_test_dir("tool-denial-record-bad");
        let session = root.join("default");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&session);

        assert_eq!(
            record_tool_execution_denial_to_session(
                &session,
                "run-1",
                "bad/tool",
                ToolExecutionDenial::InvalidToolName,
            ),
            Err(SocketSessionRecordError::InvalidField("tool"))
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_result_recorder_appends_inspectable_tool_message_and_event() {
        let root = unique_test_dir("tool-result-record");
        let session = root.join("default");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&session);
        write_text_file(
            &session.join("messages.jsonl"),
            "{\"role\":\"user\",\"content\":\"read README\"}\n",
        );
        write_text_file(
            &session.join("events.jsonl"),
            "{\"type\":\"start\",\"id\":\"run-1\",\"run\":\"run-1\"}\n",
        );

        let recorded = record_tool_execution_result_to_session(
            &session,
            "run-1",
            "call-1",
            "fs.read",
            "file contents",
        );
        assert!(recorded.is_ok());
        let Ok(recorded) = recorded else { return };
        assert_eq!(recorded.messages().len(), 1);
        assert_eq!(recorded.events().len(), 1);

        let messages = fs::read_to_string(session.join("messages.jsonl"));
        assert!(messages.is_ok());
        let Ok(messages) = messages else { return };
        let events = fs::read_to_string(session.join("events.jsonl"));
        assert!(events.is_ok());
        let Ok(events) = events else { return };

        assert!(inspect_message_stream_jsonl(&messages).is_ok());
        assert!(inspect_event_stream_jsonl(&events).is_ok());
        assert!(messages.contains("\"role\":\"tool\""));
        assert!(messages.contains("\"type\":\"tool_result\""));
        assert!(messages.contains("\"tool_call_id\":\"call-1\""));
        assert!(events.contains("\"name\":\"fs.read\""));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_result_recorder_rejects_invalid_fields_without_executing() {
        let root = unique_test_dir("tool-result-record-bad");
        let session = root.join("default");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&session);

        assert_eq!(
            record_tool_execution_result_to_session(
                &session, "run-1", "call-1", "bad/tool", "content",
            ),
            Err(SocketSessionRecordError::InvalidField("tool"))
        );
        assert_eq!(
            record_tool_execution_result_to_session(
                &session,
                "run-1",
                "call-1",
                "fs.read",
                "bad\0content",
            ),
            Err(SocketSessionRecordError::InvalidField("content"))
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn socket_session_recorder_rejects_temp_resume_and_mismatched_sessions() {
        let root = unique_test_dir("socket-session-reject");
        let session = root.join("default");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&session);

        let temp = parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","session":"default","scope":"temp","input":"hello"}"#,
        );
        assert!(temp.is_ok());
        let Ok(temp) = temp else { return };
        assert_eq!(
            record_socket_request_to_session(&session, &temp),
            Err(SocketSessionRecordError::TempSessionNotDurable)
        );

        let resume = parse_socket_request_frame(r#"{"op":"resume","session":"default"}"#);
        assert!(resume.is_ok());
        let Ok(resume) = resume else { return };
        assert_eq!(
            record_socket_request_to_session(&session, &resume),
            Err(SocketSessionRecordError::UnsupportedRequest)
        );

        let mismatch = parse_socket_request_frame(
            r#"{"op":"send","id":"msg-2","session":"other","input":"hello"}"#,
        );
        assert!(mismatch.is_ok());
        let Ok(mismatch) = mismatch else { return };
        assert_eq!(
            record_socket_request_to_session(&session, &mismatch),
            Err(SocketSessionRecordError::SessionMismatch)
        );
        assert_eq!(SocketSessionRecordError::SessionMismatch.errno(), "EINVAL");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn indexed_socket_send_records_history_and_updates_session_index() {
        let root = unique_test_dir("indexed-socket-send");
        let session_root = root.join("session");
        let session = session_root.join("default");
        let previous = session_root.join("review-1");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&session);
        create_complete_session_layout(&previous);
        write_text_file(&session.join("messages.jsonl"), "");
        write_text_file(&session.join("events.jsonl"), "");
        write_text_file(
            &session_root.join("index").join("list"),
            "review-1\ndefault\n",
        );
        write_text_file(&session_root.join("index").join("current"), "review-1\n");
        assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());

        let request = parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
        );
        assert!(request.is_ok());
        let Ok(request) = request else { return };
        let recorded = record_indexed_socket_send_to_session(&session_root, &request);
        assert!(recorded.is_ok());
        let Ok(recorded) = recorded else { return };
        assert_eq!(recorded.messages().len(), 1);
        assert_eq!(recorded.events().len(), 1);

        let by_cwd_key = session_index_key_for_cwd("/work/project");
        assert!(by_cwd_key.is_some());
        let Some(by_cwd_key) = by_cwd_key else { return };
        let messages = fs::read_to_string(session.join("messages.jsonl"));
        assert!(messages.is_ok());
        let Ok(messages) = messages else { return };
        let events = fs::read_to_string(session.join("events.jsonl"));
        assert!(events.is_ok());
        let Ok(events) = events else { return };
        let list = fs::read_to_string(session_root.join("index").join("list"));
        assert!(list.is_ok());
        let Ok(list) = list else { return };
        let current = fs::read_to_string(session_root.join("index").join("current"));
        assert!(current.is_ok());
        let Ok(current) = current else { return };
        let by_cwd = fs::read_to_string(session_root.join("index").join("by-cwd").join(by_cwd_key));
        assert!(by_cwd.is_ok());
        let Ok(by_cwd) = by_cwd else { return };

        assert!(messages.contains("\"role\":\"user\""));
        assert!(events.contains("\"type\":\"start\""));
        assert_eq!(list, "default\nreview-1\n");
        assert_eq!(current, "default\n");
        assert_eq!(by_cwd, "default\n");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn indexed_socket_send_rejects_non_send_requests() {
        let root = unique_test_dir("indexed-socket-non-send");
        let session_root = root.join("session");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

        let resume = parse_socket_request_frame(r#"{"op":"resume","session":"default"}"#);
        assert!(resume.is_ok());
        let Ok(resume) = resume else { return };
        assert_eq!(
            record_indexed_socket_send_to_session(&session_root, &resume),
            Err(IndexedSocketSessionRecordError::Session(
                SocketSessionRecordError::UnsupportedRequest
            ))
        );

        let cancel = parse_socket_request_frame(r#"{"op":"cancel","id":"run-1"}"#);
        assert!(cancel.is_ok());
        let Ok(cancel) = cancel else { return };
        assert_eq!(
            record_indexed_socket_send_to_session(&session_root, &cancel),
            Err(IndexedSocketSessionRecordError::Session(
                SocketSessionRecordError::UnsupportedRequest
            ))
        );

        let ping = parse_socket_request_frame(r#"{"op":"ping"}"#);
        assert!(ping.is_ok());
        let Ok(ping) = ping else { return };
        assert_eq!(
            record_indexed_socket_send_to_session(&session_root, &ping),
            Err(IndexedSocketSessionRecordError::Session(
                SocketSessionRecordError::UnsupportedRequest
            ))
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn indexed_socket_send_rejects_temp_sessions_before_index_update() {
        let root = unique_test_dir("indexed-socket-temp");
        let session_root = root.join("session");
        let session = session_root.join("default");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&session);
        write_text_file(&session_root.join("index").join("list"), "default\n");
        write_text_file(&session_root.join("index").join("current"), "default\n");
        assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());

        let temp = parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","session":"default","scope":"temp","cwd":"/work","input":"hello"}"#,
        );
        assert!(temp.is_ok());
        let Ok(temp) = temp else { return };
        assert_eq!(
            record_indexed_socket_send_to_session(&session_root, &temp),
            Err(IndexedSocketSessionRecordError::Session(
                SocketSessionRecordError::TempSessionNotDurable
            ))
        );
        let list = fs::read_to_string(session_root.join("index").join("list"));
        assert!(list.is_ok());
        let Ok(list) = list else { return };
        assert_eq!(list, "default\n");
        assert!(
            !session_root
                .join("index")
                .join("by-cwd")
                .join("cwd")
                .exists()
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn durable_session_layout_helper_creates_inspectable_session_and_index() {
        let root = unique_test_dir("durable-session-layout");
        let session_root = root.join("session");
        let session = session_root.join("default");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

        let ensured = ensure_durable_session_layout(
            &session_root,
            "default",
            "/work/project",
            Some("qwen"),
            SocketSessionScope::Private,
        );
        assert_eq!(ensured, Ok(()));
        assert!(inspect_session_layout(&session).is_ok());

        let list = fs::read_to_string(session_root.join("index").join("list"));
        assert!(list.is_ok());
        let Ok(list) = list else { return };
        let current = fs::read_to_string(session_root.join("index").join("current"));
        assert!(current.is_ok());
        let Ok(current) = current else { return };
        let meta = fs::read_to_string(session.join("meta.json"));
        assert!(meta.is_ok());
        let Ok(meta) = meta else { return };
        let pack = fs::read_to_string(session.join("context").join("pack.json"));
        assert!(pack.is_ok());
        let Ok(pack) = pack else { return };

        assert_eq!(list, "default\n");
        assert_eq!(current, "default\n");
        assert!(meta.contains("\"model\":\"qwen\""));
        assert!(meta.contains("\"scope\":\"private\""));
        assert!(inspect_context_pack_json(&pack).is_ok());

        let request = parse_socket_request_frame(
            r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
        );
        assert!(request.is_ok());
        let Ok(request) = request else { return };
        assert!(record_indexed_socket_send_to_session(&session_root, &request).is_ok());
        let state = fs::read_to_string(session.join("state"));
        assert!(state.is_ok());
        let Ok(state) = state else { return };
        assert_eq!(state, "active\n");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn durable_session_layout_helper_rejects_invalid_durable_inputs() {
        let root = unique_test_dir("durable-session-layout-invalid");
        let session_root = root.join("session");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

        assert_eq!(
            ensure_durable_session_layout(
                &session_root,
                "bad/name",
                "/work",
                None,
                SocketSessionScope::Private,
            ),
            Err(DurableSessionLayoutError::InvalidSessionName)
        );
        assert_eq!(
            ensure_durable_session_layout(
                &session_root,
                "default",
                "../host",
                None,
                SocketSessionScope::Private,
            ),
            Err(DurableSessionLayoutError::InvalidCwd)
        );
        assert_eq!(
            ensure_durable_session_layout(
                &session_root,
                "default",
                "/work",
                Some("bad/model"),
                SocketSessionScope::Private,
            ),
            Err(DurableSessionLayoutError::InvalidModelName)
        );
        assert_eq!(
            ensure_durable_session_layout(
                &session_root,
                "default",
                "/work",
                None,
                SocketSessionScope::Temp,
            ),
            Err(DurableSessionLayoutError::TempSessionNotDurable)
        );
        assert_eq!(DurableSessionLayoutError::InvalidCwd.errno(), "EINVAL");
        assert!(!session_root.exists());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn socket_runtime_handles_ping_send_resume_and_cancel() {
        let root = unique_test_dir("socket-runtime");
        let session_root = root.join("session");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

        let ping =
            handle_socket_request_frame(&session_root, "/work", Some("qwen"), r#"{"op":"ping"}"#);
        assert!(ping.is_ok());
        let Ok(ping) = ping else { return };
        assert_eq!(ping.jsonl(), "{\"type\":\"pong\"}\n");

        let send = handle_socket_request_frame(
            &session_root,
            "/work",
            Some("qwen"),
            r#"{"op":"send","id":"msg-1","session":"default","input":"hello"}"#,
        );
        assert!(send.is_ok());
        let Ok(send) = send else { return };
        assert_eq!(send.frames().len(), 1);
        assert!(send.jsonl().contains("\"type\":\"start\""));
        assert!(send.jsonl().contains("\"run\":\"msg-1\""));
        assert!(inspect_session_layout(&session_root.join("default")).is_ok());

        let second = handle_socket_request_frame(
            &session_root,
            "/work",
            Some("qwen"),
            r#"{"op":"send","id":"msg-2","session":"default","input":"again"}"#,
        );
        assert!(second.is_ok());

        let resume_all = handle_socket_request_frame(
            &session_root,
            "/work",
            Some("qwen"),
            r#"{"op":"resume","session":"default"}"#,
        );
        assert!(resume_all.is_ok());
        let Ok(resume_all) = resume_all else { return };
        assert_eq!(resume_all.frames().len(), 2);
        assert!(resume_all.jsonl().contains("\"run\":\"msg-1\""));
        assert!(resume_all.jsonl().contains("\"run\":\"msg-2\""));

        let resume_after = handle_socket_request_frame(
            &session_root,
            "/work",
            Some("qwen"),
            r#"{"op":"resume","session":"default","after":"msg-1"}"#,
        );
        assert!(resume_after.is_ok());
        let Ok(resume_after) = resume_after else {
            return;
        };
        assert_eq!(resume_after.frames().len(), 1);
        assert!(!resume_after.jsonl().contains("\"run\":\"msg-1\""));
        assert!(resume_after.jsonl().contains("\"run\":\"msg-2\""));

        let cancel = handle_socket_request_frame(
            &session_root,
            "/work",
            Some("qwen"),
            r#"{"op":"cancel","id":"msg-2"}"#,
        );
        assert!(cancel.is_ok());
        let Ok(cancel) = cancel else { return };
        assert!(cancel.jsonl().contains("\"status\":\"cancelled\""));
        let state = fs::read_to_string(session_root.join("default").join("state"));
        assert!(state.is_ok());
        let Ok(state) = state else { return };
        assert_eq!(state, "cancelled\n");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn socket_runtime_temp_send_does_not_create_durable_session() {
        let root = unique_test_dir("socket-runtime-temp");
        let session_root = root.join("session");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

        let send = handle_socket_request_frame(
            &session_root,
            "/work",
            Some("qwen"),
            r#"{"op":"send","id":"msg-1","session":"scratch","scope":"temp","input":"hello"}"#,
        );
        assert!(send.is_ok());
        let Ok(send) = send else { return };
        assert_eq!(send.frames().len(), 1);
        assert!(send.jsonl().contains("\"type\":\"start\""));
        assert!(send.jsonl().contains("\"model\":\"qwen\""));
        assert!(!session_root.exists());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn socket_runtime_errors_convert_to_stable_error_frames() {
        let root = unique_test_dir("socket-runtime-error");
        let session_root = root.join("session");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

        let error = handle_socket_request_frame(
            &session_root,
            "/work/../bad",
            Some("qwen"),
            r#"{"op":"send","id":"msg-1","session":"default","input":"hello"}"#,
        );
        assert_eq!(
            error,
            Err(SocketRuntimeError::SessionLayout(
                DurableSessionLayoutError::InvalidCwd
            ))
        );
        let Err(error) = error else { return };
        let response = socket_runtime_error_response(&error);
        assert_eq!(
            response.jsonl(),
            "{\"code\":\"EINVAL\",\"message\":\"EINVAL\",\"type\":\"error\"}\n"
        );
        let Some(frame) = response.frames().first() else {
            return;
        };
        let parsed = serde_json::from_str::<serde_json::Value>(frame);
        assert!(parsed.is_ok());
        let Ok(parsed) = parsed else { return };
        assert_eq!(
            parsed.get("type").and_then(serde_json::Value::as_str),
            Some("error")
        );
        assert_eq!(
            parsed.get("code").and_then(serde_json::Value::as_str),
            Some("EINVAL")
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn socket_stream_runtime_serves_one_frame_with_peer_credentials() {
        let root = unique_test_dir("socket-stream-runtime");
        let session_root = root.join("session");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        let pair = UnixStream::pair();
        assert!(pair.is_ok());
        let Ok((mut client, mut socket)) = pair else {
            return;
        };
        let peer = peer_credentials(&socket);
        assert!(peer.is_ok());
        let Ok(peer) = peer else { return };
        let policy = SocketPeerPolicy::uid_gid(peer.uid(), peer.gid());

        assert!(
            client
                .write_all(
                    br#"{"op":"send","id":"msg-1","session":"default","input":"hello"}
"#,
                )
                .is_ok()
        );
        assert!(client.shutdown(Shutdown::Write).is_ok());

        let outcome = serve_unix_socket_stream_once(
            &mut socket,
            Some(policy),
            &session_root,
            "/work",
            Some("qwen"),
        );
        assert!(outcome.is_ok());
        let Ok(outcome) = outcome else { return };
        assert_eq!(outcome.frames().len(), 1);

        let mut buffer = [0_u8; 256];
        let read = client.read(&mut buffer);
        assert!(read.is_ok());
        let Ok(read) = read else { return };
        let Some(bytes) = buffer.get(..read) else {
            return;
        };
        let response = String::from_utf8_lossy(bytes);
        assert!(response.contains("\"type\":\"start\""));
        assert!(response.contains("\"run\":\"msg-1\""));
        assert!(inspect_session_layout(&session_root.join("default")).is_ok());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn socket_stream_runtime_denies_wrong_peer_before_mutating_session() {
        let root = unique_test_dir("socket-stream-runtime-deny");
        let session_root = root.join("session");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        let pair = UnixStream::pair();
        assert!(pair.is_ok());
        let Ok((mut client, mut socket)) = pair else {
            return;
        };
        let peer = peer_credentials(&socket);
        assert!(peer.is_ok());
        let Ok(peer) = peer else { return };
        let denied_uid = if peer.uid() == u32::MAX {
            peer.uid() - 1
        } else {
            peer.uid() + 1
        };
        let policy = SocketPeerPolicy::uid(denied_uid);

        assert!(
            client
                .write_all(
                    br#"{"op":"send","id":"msg-1","session":"default","input":"hello"}
"#,
                )
                .is_ok()
        );
        assert!(client.shutdown(Shutdown::Write).is_ok());

        let outcome = serve_unix_socket_stream_once(
            &mut socket,
            Some(policy),
            &session_root,
            "/work",
            Some("qwen"),
        );
        assert_eq!(outcome, Err(SocketRuntimeError::PeerDenied));

        let mut buffer = [0_u8; 256];
        let read = client.read(&mut buffer);
        assert!(read.is_ok());
        let Ok(read) = read else { return };
        let Some(bytes) = buffer.get(..read) else {
            return;
        };
        let response = String::from_utf8_lossy(bytes);
        assert!(response.contains("\"type\":\"error\""));
        assert!(response.contains("\"code\":\"EACCES\""));
        assert!(!session_root.exists());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn socket_listener_runtime_accepts_and_serves_one_connection() {
        let root = unique_test_dir("socket-listener-runtime");
        let session_root = root.join("session");
        let socket_path = root.join("agent.sock");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(&root).is_ok());
        let listener = UnixListener::bind(&socket_path);
        assert!(listener.is_ok());
        let Ok(listener) = listener else { return };

        let client = UnixStream::connect(&socket_path);
        assert!(client.is_ok());
        let Ok(mut client) = client else { return };
        assert!(
            client
                .write_all(
                    br#"{"op":"send","id":"msg-1","session":"default","input":"hello"}
"#,
                )
                .is_ok()
        );
        assert!(client.shutdown(Shutdown::Write).is_ok());

        let outcome =
            serve_unix_socket_listener_once(&listener, None, &session_root, "/work", Some("qwen"));
        assert!(outcome.is_ok());
        let Ok(outcome) = outcome else { return };
        assert_eq!(outcome.frames().len(), 1);

        let mut buffer = [0_u8; 256];
        let read = client.read(&mut buffer);
        assert!(read.is_ok());
        let Ok(read) = read else { return };
        let Some(bytes) = buffer.get(..read) else {
            return;
        };
        let response = String::from_utf8_lossy(bytes);
        assert!(response.contains("\"type\":\"start\""));
        assert!(response.contains("\"run\":\"msg-1\""));
        assert!(inspect_session_layout(&session_root.join("default")).is_ok());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn agent_executable_socket_runtime_returns_visible_message() {
        let root = unique_test_dir("agent-executable-socket-runtime");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(ensure_v1_reference_tree(&root).is_ok());
        let session_root = root
            .join("home")
            .join("1000")
            .join("agent")
            .join("coder")
            .join("session");
        let agent_executable = root.join("agent").join("coder");
        let pair = UnixStream::pair();
        assert!(pair.is_ok());
        let Ok((mut client, mut socket)) = pair else {
            return;
        };

        assert!(
            client
                .write_all(
                    br#"{"op":"send","id":"msg-1","session":"default","input":"hi"}
"#,
                )
                .is_ok()
        );
        assert!(client.shutdown(Shutdown::Write).is_ok());

        let outcome = serve_agent_executable_socket_stream_once(
            &mut socket,
            None,
            AgentExecutableSocketRuntime {
                session_root: &session_root,
                default_cwd: "/work",
                model: Some("qwen"),
                agent_name: "coder",
                agent_executable: &agent_executable,
            },
        );
        assert!(outcome.is_ok());
        let Ok(outcome) = outcome else { return };
        assert_eq!(outcome.frames().len(), 3);
        assert!(outcome.jsonl().contains("\"type\":\"start\""));
        assert!(outcome.jsonl().contains("\"type\":\"message\""));
        assert!(outcome.jsonl().contains("\"text\":\"hi\""));
        assert!(outcome.jsonl().contains("\"type\":\"done\""));

        let mut buffer = [0_u8; 512];
        let read = client.read(&mut buffer);
        assert!(read.is_ok());
        let Ok(read) = read else { return };
        let Some(bytes) = buffer.get(..read) else {
            return;
        };
        let response = String::from_utf8_lossy(bytes);
        assert!(response.contains("\"type\":\"message\""));
        assert!(response.contains("\"text\":\"hi\""));
        let latest = fs::read_to_string(session_root.join("default").join("latest.md"));
        assert!(latest.is_ok());
        assert_eq!(latest.unwrap_or_default(), "hi\n");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn policy_v0_allows_only_exact_rules() {
        let parsed = PolicyV0::parse(
            "\
allow coder_t tool:fs.read execute
allow coder_t model:qwen use
allow coder_t shared:project-a read
",
        );
        assert!(parsed.is_ok());
        let Ok(policy) = parsed else { return };

        assert!(policy.allows(
            "coder_t",
            PolicyObjectClass::Tool,
            "fs.read",
            PolicyPermission::Execute
        ));
        assert!(policy.allows(
            "coder_t",
            PolicyObjectClass::Model,
            "qwen",
            PolicyPermission::Use
        ));
        assert!(!policy.allows(
            "coder_t",
            PolicyObjectClass::Tool,
            "shell.exec",
            PolicyPermission::Execute
        ));
        assert!(!policy.allows(
            "reviewer_t",
            PolicyObjectClass::Tool,
            "fs.read",
            PolicyPermission::Execute
        ));
        assert!(!policy.allows(
            "coder_t",
            PolicyObjectClass::Shared,
            "project-a",
            PolicyPermission::Write
        ));
    }

    #[test]
    fn policy_v0_checks_child_authority_subset() {
        let parent = PolicyV0::parse(
            "\
allow coder_t tool:fs.read execute
allow coder_t model:qwen use
allow coder_t shared:project-a read
allow coder_t session:default resume
",
        );
        assert!(parent.is_ok());
        let Ok(parent) = parent else { return };

        let child = PolicyV0::parse(
            "\
allow reviewer_t tool:fs.read execute
allow reviewer_t model:qwen use
allow reviewer_t shared:project-a read
",
        );
        assert!(child.is_ok());
        let Ok(child) = child else { return };
        assert!(child.is_authority_subset_of(&parent, "reviewer_t", "coder_t"));
        assert!(!child.is_exact_subset_of(&parent));

        let expanded_tool = PolicyV0::parse(
            "\
allow reviewer_t tool:shell.exec execute
",
        );
        assert!(expanded_tool.is_ok());
        let Ok(expanded_tool) = expanded_tool else {
            return;
        };
        assert!(!expanded_tool.is_authority_subset_of(&parent, "reviewer_t", "coder_t"));

        let wrong_subject = PolicyV0::parse(
            "\
allow other_t tool:fs.read execute
",
        );
        assert!(wrong_subject.is_ok());
        let Ok(wrong_subject) = wrong_subject else {
            return;
        };
        assert!(!wrong_subject.is_authority_subset_of(&parent, "reviewer_t", "coder_t"));
    }

    #[test]
    fn policy_v0_rejects_invalid_rules() {
        assert_eq!(
            PolicyRule::parse("deny coder_t tool:fs.read execute"),
            Err(PolicyError::ExpectedAllow)
        );
        assert_eq!(
            PolicyRule::parse("allow coder_t provider:openai use"),
            Err(PolicyError::UnknownClass)
        );
        assert_eq!(
            PolicyRule::parse("allow coder_t tool:fs.read use"),
            Err(PolicyError::UnknownPermission)
        );
        assert_eq!(
            PolicyRule::parse("allow coder_t tool:* execute"),
            Err(PolicyError::InvalidName)
        );
        assert_eq!(
            PolicyRule::parse("allow coder_t tool:fs.read execute extra"),
            Err(PolicyError::WrongFieldCount)
        );
    }

    #[test]
    fn mount_table_parses_fixed_v0_format() {
        let parsed = MountTable::parse(
            "\
/ctx\t/ctx\tro\trbind,nosuid,nodev,noexec
/home/me/project\t/work\trw\trbind,nosuid,nodev
/tmp\t/tmp\trw\t-
",
        );
        assert!(parsed.is_ok());
        let Ok(table) = parsed else { return };
        assert_eq!(table.entries().len(), 3);

        let Some(first) = table.entries().first() else {
            return;
        };
        assert_eq!(first.source(), "/ctx");
        assert_eq!(first.target(), "/ctx");
        assert_eq!(first.mode(), MountMode::ReadOnly);
        assert_eq!(
            first.options(),
            [
                MountOption::RecursiveBind,
                MountOption::NoSuid,
                MountOption::NoDev,
                MountOption::NoExec
            ]
        );

        let Some(last) = table.entries().last() else {
            return;
        };
        assert!(last.options().is_empty());
    }

    #[test]
    fn mount_table_checks_child_attenuation() {
        let parent = MountTable::parse(
            "\
/home/me/project\t/work\trw\trbind,nosuid,nodev
/ctx/shared/project-a\t/shared/project-a\tro\trbind,nosuid,nodev,noexec
",
        );
        assert!(parent.is_ok());
        let Ok(parent) = parent else { return };

        let narrowed = MountTable::parse(
            "\
/home/me/project\t/work\tro\tbind,nosuid,nodev,noexec
/ctx/shared/project-a\t/shared/project-a\tro\tbind,nosuid,nodev,noexec
",
        );
        assert!(narrowed.is_ok());
        let Ok(narrowed) = narrowed else { return };
        assert!(narrowed.is_subset_of(&parent));

        let write_expansion = MountTable::parse(
            "\
/ctx/shared/project-a\t/shared/project-a\trw\tbind,nosuid,nodev,noexec
",
        );
        assert!(write_expansion.is_ok());
        let Ok(write_expansion) = write_expansion else {
            return;
        };
        assert!(!write_expansion.is_subset_of(&parent));

        let removed_safety = MountTable::parse(
            "\
/ctx/shared/project-a\t/shared/project-a\tro\tbind,nosuid,nodev
",
        );
        assert!(removed_safety.is_ok());
        let Ok(removed_safety) = removed_safety else {
            return;
        };
        assert!(!removed_safety.is_subset_of(&parent));

        let hidden_parent_path = MountTable::parse(
            "\
/secret\t/secret\tro\tbind,nosuid,nodev,noexec
",
        );
        assert!(hidden_parent_path.is_ok());
        let Ok(hidden_parent_path) = hidden_parent_path else {
            return;
        };
        assert!(!hidden_parent_path.is_subset_of(&parent));
    }

    #[test]
    fn mount_table_rejects_invalid_v0_format() {
        assert_eq!(
            MountEntry::parse("ctx\t/ctx\tro\trbind"),
            Err(MountError::InvalidPath)
        );
        assert_eq!(
            MountEntry::parse("/ctx\tctx\tro\trbind"),
            Err(MountError::InvalidPath)
        );
        assert_eq!(
            MountEntry::parse("/ctx\t/ctx\tbad\trbind"),
            Err(MountError::InvalidMode)
        );
        assert_eq!(
            MountEntry::parse("/ctx\t/ctx\tro\tbind,rbind"),
            Err(MountError::ConflictingBindOption)
        );
        assert_eq!(
            MountEntry::parse("/ctx\t/ctx\tro\trbind,rbind"),
            Err(MountError::DuplicateOption)
        );
        assert_eq!(
            MountEntry::parse("/ctx\t/ctx\tro\tdev"),
            Err(MountError::InvalidOption)
        );
        assert_eq!(
            MountEntry::parse("/ctx\t/ctx\tro"),
            Err(MountError::WrongFieldCount)
        );
    }

    #[test]
    fn child_agent_authority_accepts_attenuated_owned_child() {
        let parent_identity = AgentUnixIdentity::new(1000, 100, [10, 20, 30]);
        let child_identity = AgentUnixIdentity::new(1000, 100, [10, 30]);
        let parent_policy = PolicyV0::parse(
            "\
allow coder_t tool:fs.read execute
allow coder_t model:qwen use
allow coder_t shared:project-a read
",
        );
        assert!(parent_policy.is_ok());
        let Ok(parent_policy) = parent_policy else {
            return;
        };
        let child_policy = PolicyV0::parse(
            "\
allow reviewer_t tool:fs.read execute
allow reviewer_t shared:project-a read
",
        );
        assert!(child_policy.is_ok());
        let Ok(child_policy) = child_policy else {
            return;
        };
        let parent_mounts = MountTable::parse(
            "\
/work\t/work\trw\trbind,nosuid,nodev
/ctx/shared/project-a\t/shared/project-a\tro\trbind,nosuid,nodev,noexec
",
        );
        assert!(parent_mounts.is_ok());
        let Ok(parent_mounts) = parent_mounts else {
            return;
        };
        let child_mounts = MountTable::parse(
            "\
/work\t/work\tro\tbind,nosuid,nodev,noexec
/ctx/shared/project-a\t/shared/project-a\tro\tbind,nosuid,nodev,noexec
",
        );
        assert!(child_mounts.is_ok());
        let Ok(child_mounts) = child_mounts else {
            return;
        };

        let request = ChildAgentRequest::new(
            "reviewer",
            "agent:coder session:default run:r123",
            ChildLifecycle::Owned,
            ChildAgentControls::new(&child_identity, "reviewer_t", &child_policy, &child_mounts),
        );
        let authority = ChildAgentAuthority::new(
            "coder",
            &parent_identity,
            "coder_t",
            &parent_policy,
            &parent_mounts,
        );
        assert_eq!(authorize_child_agent(request, authority), Ok(()));
    }

    #[test]
    fn child_agent_authority_rejects_identity_group_policy_and_mount_expansion() {
        let parent_identity = AgentUnixIdentity::new(1000, 100, [10]);
        let child_identity = AgentUnixIdentity::new(1000, 100, [10]);
        let expanded_identity = AgentUnixIdentity::new(1001, 100, [10]);
        let expanded_groups = AgentUnixIdentity::new(1000, 100, [10, 20]);
        let parent_policy = allow_tool_policy("coder_t", "fs.read");
        let child_policy = allow_tool_policy("reviewer_t", "fs.read");
        let expanded_policy = allow_tool_policy("reviewer_t", "shell.exec");
        let parent_mounts = MountTable::parse("/work\t/work\tro\tbind,nosuid,nodev,noexec\n");
        assert!(parent_mounts.is_ok());
        let Ok(parent_mounts) = parent_mounts else {
            return;
        };
        let child_mounts = MountTable::parse("/work\t/work\tro\tbind,nosuid,nodev,noexec\n");
        assert!(child_mounts.is_ok());
        let Ok(child_mounts) = child_mounts else {
            return;
        };
        let expanded_mounts = MountTable::parse("/work\t/work\trw\tbind,nosuid,nodev,noexec\n");
        assert!(expanded_mounts.is_ok());
        let Ok(expanded_mounts) = expanded_mounts else {
            return;
        };
        let authority = ChildAgentAuthority::new(
            "coder",
            &parent_identity,
            "coder_t",
            &parent_policy,
            &parent_mounts,
        );

        let base = ChildAgentRequest::new(
            "reviewer",
            "agent:coder",
            ChildLifecycle::Owned,
            ChildAgentControls::new(&child_identity, "reviewer_t", &child_policy, &child_mounts),
        );
        assert_eq!(authorize_child_agent(base, authority), Ok(()));

        let identity_request = ChildAgentRequest::new(
            "reviewer",
            "agent:coder",
            ChildLifecycle::Owned,
            ChildAgentControls::new(
                &expanded_identity,
                "reviewer_t",
                &child_policy,
                &child_mounts,
            ),
        );
        assert_eq!(
            authorize_child_agent(identity_request, authority),
            Err(ChildAgentDenial::IdentityExpansion)
        );

        let group_request = ChildAgentRequest::new(
            "reviewer",
            "agent:coder",
            ChildLifecycle::Owned,
            ChildAgentControls::new(&expanded_groups, "reviewer_t", &child_policy, &child_mounts),
        );
        assert_eq!(
            authorize_child_agent(group_request, authority),
            Err(ChildAgentDenial::GroupExpansion)
        );

        let policy_request = ChildAgentRequest::new(
            "reviewer",
            "agent:coder",
            ChildLifecycle::Owned,
            ChildAgentControls::new(
                &child_identity,
                "reviewer_t",
                &expanded_policy,
                &child_mounts,
            ),
        );
        assert_eq!(
            authorize_child_agent(policy_request, authority),
            Err(ChildAgentDenial::PolicyExpansion)
        );

        let mount_request = ChildAgentRequest::new(
            "reviewer",
            "agent:coder",
            ChildLifecycle::Owned,
            ChildAgentControls::new(
                &child_identity,
                "reviewer_t",
                &child_policy,
                &expanded_mounts,
            ),
        );
        assert_eq!(
            authorize_child_agent(mount_request, authority),
            Err(ChildAgentDenial::MountExpansion)
        );
    }

    #[test]
    fn child_agent_authority_rejects_bad_parent_reference_and_lifecycle() {
        let parent_identity = AgentUnixIdentity::new(1000, 100, [10]);
        let child_identity = AgentUnixIdentity::new(1000, 100, [10]);
        let parent_policy = allow_tool_policy("coder_t", "fs.read");
        let child_policy = allow_tool_policy("reviewer_t", "fs.read");
        let parent_mounts = MountTable::parse("/work\t/work\tro\tbind,nosuid,nodev,noexec\n");
        assert!(parent_mounts.is_ok());
        let Ok(parent_mounts) = parent_mounts else {
            return;
        };
        let child_mounts = MountTable::parse("/work\t/work\tro\tbind,nosuid,nodev,noexec\n");
        assert!(child_mounts.is_ok());
        let Ok(child_mounts) = child_mounts else {
            return;
        };
        let authority = ChildAgentAuthority::new(
            "coder",
            &parent_identity,
            "coder_t",
            &parent_policy,
            &parent_mounts,
        );

        let mismatch = ChildAgentRequest::new(
            "reviewer",
            "agent:planner",
            ChildLifecycle::Owned,
            ChildAgentControls::new(&child_identity, "reviewer_t", &child_policy, &child_mounts),
        );
        assert_eq!(
            authorize_child_agent(mismatch, authority),
            Err(ChildAgentDenial::ParentMismatch)
        );

        let bad_ref = ChildAgentRequest::new(
            "reviewer",
            "parent:coder",
            ChildLifecycle::Owned,
            ChildAgentControls::new(&child_identity, "reviewer_t", &child_policy, &child_mounts),
        );
        assert_eq!(
            authorize_child_agent(bad_ref, authority),
            Err(ChildAgentDenial::InvalidParentRef)
        );

        assert_eq!(
            ChildLifecycle::parse("detached"),
            Err(ChildAgentDenial::UnsupportedLifecycle)
        );
    }

    #[test]
    fn owned_child_cancellation_records_state_and_events_without_deleting_history() {
        let root = unique_test_dir("owned-child-cancel");
        let parent_session = root.join("home").join("1000").join("agent").join("coder");
        let child_session = root.join("home").join("1000").join("agent").join("rev-123");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        write_text_file(&parent_session.join("events.jsonl"), "");
        create_complete_session_layout(&child_session);
        write_text_file(
            &child_session.join("messages.jsonl"),
            "{\"role\":\"user\",\"content\":\"review this\"}\n",
        );
        write_text_file(&child_session.join("events.jsonl"), "");

        let recorded =
            record_owned_child_cancellation("coder", "rev-123", &parent_session, &child_session);
        assert!(recorded.is_ok());
        let Ok(events) = recorded else { return };
        let child_state = fs::read_to_string(child_session.join("state"));
        assert!(child_state.is_ok());
        let Ok(child_state) = child_state else { return };
        assert_eq!(child_state, "cancelled\n");
        let child_messages = fs::read_to_string(child_session.join("messages.jsonl"));
        assert!(child_messages.is_ok());
        let Ok(child_messages) = child_messages else {
            return;
        };
        assert_eq!(
            child_messages,
            "{\"role\":\"user\",\"content\":\"review this\"}\n"
        );

        let parent_events = fs::read_to_string(parent_session.join("events.jsonl"));
        assert!(parent_events.is_ok());
        let Ok(parent_events) = parent_events else {
            return;
        };
        let child_events = fs::read_to_string(child_session.join("events.jsonl"));
        assert!(child_events.is_ok());
        let Ok(child_events) = child_events else {
            return;
        };
        assert_eq!(parent_events, format!("{}\n", events.parent_event()));
        assert_eq!(child_events, format!("{}\n", events.child_event()));
        assert!(inspect_event_stream_jsonl(&events.jsonl()).is_ok());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn owned_child_cancellation_rejects_bad_names_and_missing_history() {
        let root = unique_test_dir("owned-child-cancel-bad");
        let parent_session = root.join("parent");
        let child_session = root.join("child");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        write_text_file(&parent_session.join("events.jsonl"), "");
        write_text_file(&child_session.join("events.jsonl"), "");
        write_text_file(&child_session.join("state"), "idle\n");

        assert_eq!(
            owned_child_cancellation_events("bad/parent", "rev-123"),
            Err(OwnedChildCancellationError::InvalidParentName)
        );
        assert_eq!(
            record_owned_child_cancellation("coder", "bad/child", &parent_session, &child_session),
            Err(OwnedChildCancellationError::InvalidChildName)
        );
        assert_eq!(
            record_owned_child_cancellation("coder", "rev-123", &parent_session, &child_session),
            Err(OwnedChildCancellationError::MissingChildHistory)
        );
        assert_eq!(
            OwnedChildCancellationError::MissingChildHistory.errno(),
            "ENOENT"
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn child_context_recorder_creates_handoff_and_result_channel() {
        let root = unique_test_dir("child-context-record");
        let session = root.join("default");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&session);

        let handoff = record_child_handoff_to_parent_context(
            &session,
            "rev-2",
            "reviewer",
            "default",
            "Task: review mount ABI\n",
        );
        assert_eq!(handoff, Ok(()));

        let child = session.join("context").join("child").join("rev-2");
        let agent = fs::read_to_string(child.join("agent"));
        assert!(agent.is_ok());
        let Ok(agent) = agent else { return };
        let status = fs::read_to_string(child.join("status"));
        assert!(status.is_ok());
        let Ok(status) = status else { return };
        let handoff = fs::read_to_string(child.join("handoff.md"));
        assert!(handoff.is_ok());
        let Ok(handoff) = handoff else { return };

        assert_eq!(agent, "reviewer\n");
        assert_eq!(status, "pending\n");
        assert_eq!(handoff, "Task: review mount ABI\n");
        assert!(validate_context_pack_source("context/child/rev-2/handoff.md").is_ok());

        let refs = r#"{"id":"r1","path":"artifact/report.md","kind":"artifact","summary":"review report"}"#;
        let result = record_child_result_to_parent_context(
            &session,
            "rev-2",
            ChildContextStatus::Done,
            "Summary: ok",
            refs,
        );
        assert_eq!(result, Ok(()));

        let result_md = fs::read_to_string(child.join("result.md"));
        assert!(result_md.is_ok());
        let Ok(result_md) = result_md else { return };
        let refs_jsonl = fs::read_to_string(child.join("refs.jsonl"));
        assert!(refs_jsonl.is_ok());
        let Ok(refs_jsonl) = refs_jsonl else {
            return;
        };
        let status = fs::read_to_string(child.join("status"));
        assert!(status.is_ok());
        let Ok(status) = status else { return };

        assert_eq!(result_md, "Summary: ok\n");
        assert_eq!(status, "done\n");
        assert!(inspect_context_jsonl(ContextJsonlKind::Refs, &refs_jsonl).is_ok());
        assert!(validate_context_pack_source("context/child/rev-2/result.md").is_ok());
        assert!(validate_context_pack_source("context/child/rev-2/refs.jsonl").is_ok());
        assert!(inspect_session_layout(&session).is_ok());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn child_context_recorder_rejects_bad_names_status_and_refs() {
        let root = unique_test_dir("child-context-record-bad");
        let session = root.join("default");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&session);

        assert_eq!(
            record_child_handoff_to_parent_context(
                &session,
                "bad/child",
                "reviewer",
                "default",
                "Task: no\n",
            ),
            Err(ChildContextRecordError::InvalidChildName)
        );
        assert_eq!(
            record_child_handoff_to_parent_context(
                &session,
                "rev-2",
                "reviewer",
                "default",
                "Task: no\n",
            ),
            Ok(())
        );
        assert_eq!(
            record_child_result_to_parent_context(
                &session,
                "rev-2",
                ChildContextStatus::Pending,
                "not terminal",
                "",
            ),
            Err(ChildContextRecordError::InvalidStatus)
        );
        assert_eq!(
            record_child_result_to_parent_context(
                &session,
                "rev-2",
                ChildContextStatus::Done,
                "done",
                "{\"path\":\"../secret\"}\n",
            ),
            Err(ChildContextRecordError::InvalidRefs)
        );
        assert_eq!(ChildContextRecordError::InvalidRefs.errno(), "EINVAL");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn session_layout_inspector_accepts_transparent_context_tree() {
        let root = unique_test_dir("session-layout-ok");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&root);

        let report = inspect_session_layout(&root);
        assert!(report.is_ok());
        assert!(report.issues().is_empty());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn session_layout_inspector_reports_missing_and_wrong_types() {
        let root = unique_test_dir("session-layout-bad");
        let context = root.join("context");
        let child = context.join("child").join("rev-1");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(root.join("messages.jsonl")).is_ok());
        assert!(fs::create_dir_all(&child).is_ok());
        assert!(fs::write(child.join("agent"), "reviewer\n").is_ok());
        assert!(fs::create_dir_all(context.join("pack.md")).is_ok());

        let report = inspect_session_layout(&root);
        assert!(!report.is_ok());
        assert!(
            report
                .issues()
                .contains(&SessionLayoutIssue::NotFile("messages.jsonl".to_owned()))
        );
        assert!(
            report
                .issues()
                .contains(&SessionLayoutIssue::MissingFile("events.jsonl".to_owned()))
        );
        assert!(
            report
                .issues()
                .contains(&SessionLayoutIssue::NotFile("context/pack.md".to_owned()))
        );
        assert!(report.issues().contains(&SessionLayoutIssue::MissingFile(
            "context/child/rev-1/result.md".to_owned()
        )));
        assert!(
            report
                .issues()
                .contains(&SessionLayoutIssue::MissingDirectory(
                    "context/child/rev-1/artifact".to_owned()
                ))
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn session_controls_accept_fixed_v1_values() {
        assert!(inspect_session_control(SessionControlKind::State, "active\n").is_ok());
        assert!(inspect_session_control(SessionControlKind::State, "cancelled\n").is_ok());
        assert!(inspect_session_control(SessionControlKind::Cwd, "/work/project\n").is_ok());
        assert!(
            inspect_session_control(
                SessionControlKind::MetaJson,
                "{\"client\":\"ctx\",\"model\":\"qwen\",\"scope\":\"shared\"}\n"
            )
            .is_ok()
        );
        assert!(inspect_session_control(SessionControlKind::MetaJson, "{}\n").is_ok());
    }

    #[test]
    fn session_controls_reject_invalid_state_cwd_and_meta() {
        assert_eq!(
            inspect_session_control(SessionControlKind::State, "running\n").issues(),
            &[SessionControlIssue::InvalidValue {
                line: 1,
                value: "running".to_owned()
            }]
        );
        assert_eq!(
            inspect_session_control(SessionControlKind::Cwd, "../work\n").issues(),
            &[SessionControlIssue::InvalidValue {
                line: 1,
                value: "../work".to_owned()
            }]
        );
        assert_eq!(
            inspect_session_control(SessionControlKind::Cwd, "/work/../secret\n").issues(),
            &[SessionControlIssue::InvalidValue {
                line: 1,
                value: "/work/../secret".to_owned()
            }]
        );
        assert_eq!(
            inspect_session_control(SessionControlKind::MetaJson, "{").issues(),
            &[SessionControlIssue::InvalidJson]
        );
        assert_eq!(
            inspect_session_control(SessionControlKind::MetaJson, "[]\n").issues(),
            &[SessionControlIssue::NotObject]
        );
        assert_eq!(
            inspect_session_control(SessionControlKind::MetaJson, "{\"scope\":\"global\"}\n")
                .issues(),
            &[SessionControlIssue::InvalidValue {
                line: 1,
                value: "global".to_owned()
            }]
        );
    }

    #[test]
    fn session_layout_inspector_rejects_invalid_control_values() {
        let root = unique_test_dir("session-layout-control-bad");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&root);
        write_text_file(&root.join("state"), "running\n");
        write_text_file(&root.join("cwd"), "/work/../secret\n");
        write_text_file(&root.join("meta.json"), "{\"model\":\"bad/model\"}\n");

        let report = inspect_session_layout(&root);
        assert!(
            report
                .issues()
                .contains(&SessionLayoutIssue::InvalidFileValue {
                    path: "state".to_owned(),
                    value: "running".to_owned()
                })
        );
        assert!(
            report
                .issues()
                .contains(&SessionLayoutIssue::InvalidFileValue {
                    path: "cwd".to_owned(),
                    value: "/work/../secret".to_owned()
                })
        );
        assert!(
            report
                .issues()
                .contains(&SessionLayoutIssue::InvalidFileValue {
                    path: "meta.json".to_owned(),
                    value: "bad/model".to_owned()
                })
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn session_index_accepts_fixed_formats() {
        assert!(inspect_session_index(SessionIndexKind::List, "default\nreview-1\n").is_ok());
        assert!(inspect_session_index(SessionIndexKind::Current, "default\n").is_ok());
        assert!(inspect_session_index(SessionIndexKind::ByCwd, "worktree-1").is_ok());
        assert!(inspect_session_index(SessionIndexKind::List, "").is_ok());
    }

    #[test]
    fn session_index_rejects_invalid_names_and_multi_value_files() {
        let list = inspect_session_index(SessionIndexKind::List, "default\nbad/name\n\n spaced\n");
        assert_eq!(
            list.issues(),
            &[
                SessionIndexIssue::InvalidSessionName {
                    line: 2,
                    value: "bad/name".to_owned()
                },
                SessionIndexIssue::EmptyValue { line: 3 },
                SessionIndexIssue::InvalidSessionName {
                    line: 4,
                    value: "spaced".to_owned()
                }
            ]
        );

        let current = inspect_session_index(SessionIndexKind::Current, "default\nother\n");
        assert_eq!(
            current.issues(),
            &[SessionIndexIssue::MultipleValues { line: 2 }]
        );

        let empty = inspect_session_index(SessionIndexKind::ByCwd, "");
        assert_eq!(empty.issues(), &[SessionIndexIssue::EmptyValue { line: 1 }]);
    }

    #[test]
    fn session_index_update_sets_current_and_deduplicated_list() {
        let root = unique_test_dir("session-index-update");
        let session_root = root.join("session");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());
        assert!(fs::create_dir_all(session_root.join("default")).is_ok());
        assert!(fs::create_dir_all(session_root.join("review-1")).is_ok());
        write_text_file(
            &session_root.join("index").join("list"),
            "default\nreview-1\n",
        );
        write_text_file(&session_root.join("index").join("current"), "default\n");

        let updated = update_session_index(&session_root, "review-1", Some("cwd-hash-1"));
        assert_eq!(updated, Ok(()));
        let list = fs::read_to_string(session_root.join("index").join("list"));
        assert!(list.is_ok());
        let Ok(list) = list else { return };
        let current = fs::read_to_string(session_root.join("index").join("current"));
        assert!(current.is_ok());
        let Ok(current) = current else { return };
        let by_cwd =
            fs::read_to_string(session_root.join("index").join("by-cwd").join("cwd-hash-1"));
        assert!(by_cwd.is_ok());
        let Ok(by_cwd) = by_cwd else { return };

        assert_eq!(list, "review-1\ndefault\n");
        assert_eq!(current, "review-1\n");
        assert_eq!(by_cwd, "review-1\n");
        assert!(inspect_session_index(SessionIndexKind::List, &list).is_ok());
        assert!(inspect_session_index(SessionIndexKind::Current, &current).is_ok());
        assert!(inspect_session_index(SessionIndexKind::ByCwd, &by_cwd).is_ok());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn session_index_update_rejects_missing_and_invalid_index_state() {
        let root = unique_test_dir("session-index-update-bad");
        let session_root = root.join("session");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());
        assert!(fs::create_dir_all(session_root.join("default")).is_ok());
        write_text_file(&session_root.join("index").join("list"), "bad/name\n");
        write_text_file(&session_root.join("index").join("current"), "default\n");

        assert_eq!(
            update_session_index(&session_root, "bad/name", None),
            Err(SessionIndexUpdateError::InvalidSessionName)
        );
        assert_eq!(
            update_session_index(&session_root, "missing", None),
            Err(SessionIndexUpdateError::MissingSession)
        );
        assert_eq!(
            update_session_index(&session_root, "default", Some("bad/key")),
            Err(SessionIndexUpdateError::InvalidByCwdKey)
        );
        assert_eq!(
            update_session_index(&session_root, "default", None),
            Err(SessionIndexUpdateError::InvalidIndex)
        );
        assert_eq!(SessionIndexUpdateError::InvalidIndex.errno(), "EINVAL");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn context_pack_sources_are_session_relative_and_inspectable() {
        let report = inspect_context_pack_json(
            r#"{
  "session": "default",
  "agent": "coder",
  "items": [
    {"kind": "summary", "source": "context/summary.md"},
    {"kind": "messages", "source": "messages.jsonl"},
    {"kind": "child_result", "source": "context/child/rev-1/result.md"},
    {"kind": "child_refs", "source": "context/child/rev-1/refs.jsonl"},
    {"kind": "artifact", "source": "context/child/rev-1/artifact/report.md"},
    {"kind": "pinned", "source": "context/pinned/system.md"}
  ]
}"#,
        );
        assert!(report.is_ok());
        assert!(validate_context_pack_source("context/facts.jsonl").is_ok());
    }

    #[test]
    fn context_pack_sources_reject_escapes_and_child_history() {
        assert_eq!(
            validate_context_pack_source(
                "/ctx/shared/im-a/agent/bot/session/group-1/messages.jsonl"
            ),
            Err(ContextPackSourceError::Absolute)
        );
        assert_eq!(
            validate_context_pack_source("../other/messages.jsonl"),
            Err(ContextPackSourceError::ParentComponent)
        );
        assert_eq!(
            validate_context_pack_source("session/other/messages.jsonl"),
            Err(ContextPackSourceError::UnsupportedSessionPath)
        );
        assert_eq!(
            validate_context_pack_source("context/child/rev-1/messages.jsonl"),
            Err(ContextPackSourceError::UnsupportedChildPath)
        );

        let report = inspect_context_pack_json(
            r#"{
  "items": [
    {"kind": "ok", "source": "context/summary.md"},
    {"kind": "absolute", "source": "/ctx/shared/im-b/agent/bot/session/channel-2/messages.jsonl"},
    {"kind": "child_full_history", "source": "context/child/rev-1/messages.jsonl"},
    {"kind": "missing"},
    {"kind": "not_string", "source": 42}
  ]
}"#,
        );
        assert!(!report.is_ok());
        assert_eq!(
            report.issues(),
            [
                ContextPackIssue::InvalidSource {
                    item: 1,
                    source: "/ctx/shared/im-b/agent/bot/session/channel-2/messages.jsonl"
                        .to_owned(),
                    reason: ContextPackSourceError::Absolute
                },
                ContextPackIssue::InvalidSource {
                    item: 2,
                    source: "context/child/rev-1/messages.jsonl".to_owned(),
                    reason: ContextPackSourceError::UnsupportedChildPath
                },
                ContextPackIssue::MissingSource(3),
                ContextPackIssue::SourceNotString(4)
            ]
        );
    }

    #[test]
    fn context_pack_rebuild_writes_inspectable_sources_without_child_history() {
        let root = unique_test_dir("context-pack-rebuild");
        let session = root.join("default");
        let context = session.join("context");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&session);
        write_text_file(
            &session.join("messages.jsonl"),
            "{\"role\":\"system\",\"content\":\"base rules\"}\n{\"role\":\"user\",\"content\":\"fix tests\"}\n{\"role\":\"assistant\",\"content\":\"working\"}\n",
        );
        write_text_file(&context.join("budget"), "0\n");
        write_text_file(
            &context.join("pinned").join("system.md"),
            "Pinned system text\n",
        );
        write_text_file(&context.join("summary.md"), "Short summary\n");
        write_text_file(
            &context.join("facts.jsonl"),
            "{\"id\":\"f1\",\"text\":\"Root ABI is frozen.\",\"source\":\"messages:1-2\"}\n",
        );
        write_text_file(
            &context.join("decisions.jsonl"),
            "{\"id\":\"d1\",\"decision\":\"Do not add provider root.\",\"source\":\"messages:3\"}\n",
        );
        write_text_file(&context.join("todo.md"), "Keep FUSE small\n");
        write_text_file(
            &context.join("refs.jsonl"),
            "{\"id\":\"r1\",\"path\":\"docs/spec/16-context.md\",\"kind\":\"file\",\"summary\":\"context spec\"}\n",
        );
        write_text_file(
            &context.join("child").join("rev-1").join("result.md"),
            "Child says ok\n",
        );
        write_text_file(
            &context.join("child").join("rev-1").join("refs.jsonl"),
            "{\"id\":\"cr1\",\"path\":\"artifact/report.md\",\"kind\":\"artifact\",\"summary\":\"child report\"}\n",
        );
        write_text_file(
            &context.join("child").join("rev-1").join("messages.jsonl"),
            "{\"role\":\"user\",\"content\":\"must not be packed\"}\n",
        );

        let built = rebuild_context_pack(&session, Some("coder"), 2);
        assert!(built.is_ok());
        let Ok(built) = built else { return };

        let pack_json = fs::read_to_string(context.join("pack.json"));
        assert!(pack_json.is_ok());
        let Ok(pack_json) = pack_json else { return };
        let pack_md = fs::read_to_string(context.join("pack.md"));
        assert!(pack_md.is_ok());
        let Ok(pack_md) = pack_md else { return };

        assert_eq!(built.pack_json(), pack_json);
        assert_eq!(built.pack_md(), pack_md);
        assert!(inspect_context_pack_json(&pack_json).is_ok());
        assert!(pack_json.contains("\"source\":\"context/pinned/system.md\""));
        assert!(pack_json.contains("\"source\":\"messages.jsonl\""));
        assert!(pack_json.contains("\"range\":\"tail:2\""));
        assert!(pack_json.contains("\"source\":\"context/child/rev-1/result.md\""));
        assert!(pack_json.contains("\"source\":\"context/child/rev-1/refs.jsonl\""));
        assert!(!pack_json.contains("context/child/rev-1/messages.jsonl"));
        assert!(pack_md.contains("Pinned system text"));
        assert!(pack_md.contains("Child says ok"));
        assert!(pack_md.contains("\"role\":\"assistant\""));
        assert!(!pack_md.contains("must not be packed"));
        assert!(built.items().iter().all(|item| {
            validate_context_pack_source(item.source()).is_ok()
                && item.source() != "context/child/rev-1/messages.jsonl"
        }));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn context_pack_rebuild_respects_budget_and_validates_inputs() {
        let root = unique_test_dir("context-pack-rebuild-budget");
        let session = root.join("default");
        let context = session.join("context");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_complete_session_layout(&session);
        write_text_file(
            &session.join("messages.jsonl"),
            "{\"role\":\"user\",\"content\":\"one two three four five six\"}\n",
        );
        write_text_file(&context.join("budget"), "2\n");
        write_text_file(&context.join("summary.md"), "one two\n");
        write_text_file(&context.join("facts.jsonl"), "");
        write_text_file(&context.join("decisions.jsonl"), "");
        write_text_file(&context.join("todo.md"), "");
        write_text_file(&context.join("refs.jsonl"), "");
        write_text_file(&context.join("child").join("rev-1").join("result.md"), "");
        write_text_file(&context.join("child").join("rev-1").join("refs.jsonl"), "");

        let built = rebuild_context_pack(&session, Some("coder"), 5);
        assert!(built.is_ok());
        let Ok(built) = built else { return };
        assert_eq!(built.items().len(), 1);
        assert_eq!(
            built
                .items()
                .first()
                .map(super::ContextPackBuiltItem::source),
            Some("context/summary.md")
        );
        assert!(!built.pack_json().contains("messages.jsonl"));

        write_text_file(&context.join("budget"), " 2\n");
        assert_eq!(
            rebuild_context_pack(&session, Some("coder"), 5),
            Err(ContextPackBuildError::InvalidBudget)
        );
        write_text_file(&context.join("budget"), "0\n");
        write_text_file(
            &session.join("messages.jsonl"),
            "{\"role\":\"native_thread\"}\n",
        );
        assert_eq!(
            rebuild_context_pack(&session, Some("coder"), 5),
            Err(ContextPackBuildError::InvalidMessages)
        );
        write_text_file(
            &session.join("messages.jsonl"),
            "{\"role\":\"user\",\"content\":\"ok\"}\n",
        );
        assert_eq!(
            rebuild_context_pack(&session, Some("bad/agent"), 5),
            Err(ContextPackBuildError::InvalidAgentName)
        );
        assert!(fs::create_dir_all(context.join("child").join(".bad")).is_ok());
        assert_eq!(
            rebuild_context_pack(&session, Some("coder"), 5),
            Err(ContextPackBuildError::InvalidChildName)
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn context_pack_rejects_invalid_json_shape() {
        assert_eq!(
            inspect_context_pack_json("{").issues(),
            &[ContextPackIssue::InvalidJson]
        );
        assert_eq!(
            inspect_context_pack_json(r#"{"items": {"source": "messages.jsonl"}}"#).issues(),
            &[ContextPackIssue::ItemsNotArray]
        );
        assert_eq!(
            inspect_context_pack_json(r#"{"items": ["messages.jsonl"]}"#).issues(),
            &[ContextPackIssue::ItemNotObject(0)]
        );
    }

    #[test]
    fn message_stream_accepts_canonical_role_content_frames() {
        let report = inspect_message_stream_jsonl(
            r#"{"role":"system","content":"You are concise."}
{"role":"user","content":[{"type":"text","text":"hello"}]}
{"role":"assistant","content":[{"type":"text","text":"hi"}]}
{"role":"tool","content":[{"type":"tool_result","tool_call_id":"call-1","content":"ok"}]}
"#,
        );
        assert!(report.is_ok());
        assert!(report.issues().is_empty());
    }

    #[test]
    fn message_stream_rejects_native_state_and_bad_shape() {
        let report = inspect_message_stream_jsonl(
            r#"not-json
[]
{"content":"missing role"}
{"role":"developer","content":"private role"}
{"role":"assistant","response_id":"resp-1","content":"hi"}
{"role":"assistant","content":[{"type":"provider_blob","text":"x"}]}
{"role":"assistant"}
"#,
        );
        assert_eq!(
            report.issues(),
            [
                MessageStreamIssue::InvalidJson(1),
                MessageStreamIssue::MessageNotObject(2),
                MessageStreamIssue::MissingRole(3),
                MessageStreamIssue::InvalidRole {
                    line: 4,
                    role: "developer".to_owned()
                },
                MessageStreamIssue::ProviderNativeField {
                    line: 5,
                    field: "response_id".to_owned()
                },
                MessageStreamIssue::InvalidContent(6),
                MessageStreamIssue::MissingContent(7)
            ]
        );
    }

    #[test]
    fn context_jsonl_accepts_spec_record_shapes() {
        assert!(
            inspect_context_jsonl(
                ContextJsonlKind::Facts,
                r#"{"id":"f1","text":"CortexFS root is small.","source":"messages:12-18"}
"#
            )
            .is_ok()
        );
        assert!(
            inspect_context_jsonl(
                ContextJsonlKind::Decisions,
                r#"{"id":"d1","decision":"Child agents are owned.","source":"user:latest"}
"#
            )
            .is_ok()
        );
        assert!(
            inspect_context_jsonl(
                ContextJsonlKind::Refs,
                r#"{"id":"r1","path":"/work/DESIGN.md","kind":"file","summary":"design"}
{"id":"r2","path":"context/swap/chunk/sha256-abc","kind":"swap","summary":"old design"}
"#
            )
            .is_ok()
        );
        assert!(
            inspect_context_jsonl(
                ContextJsonlKind::SwapIndex,
                r#"{"id":"sha256-abc","kind":"message_range","source":"messages.jsonl","summary":"initial design","tokens":18000}
{"id":"sha256-def","kind":"tool_output","source":"events.jsonl","summary":"test output","tokens":45000}
"#
            )
            .is_ok()
        );
        assert!(
            inspect_context_jsonl(
                ContextJsonlKind::DedupIndex,
                r#"{"hash":"sha256-abc","refs":["messages:1-40","swap:old-design"],"bytes":12000,"tokens":3000}
"#
            )
            .is_ok()
        );
    }

    #[test]
    fn context_jsonl_rejects_invalid_records() {
        let facts = inspect_context_jsonl(
            ContextJsonlKind::Facts,
            "not-json\n[]\n{\"id\":\"bad/id\",\"text\":\"ok\"}\n",
        );
        assert_eq!(
            facts.issues(),
            [
                ContextJsonlIssue::InvalidJson(1),
                ContextJsonlIssue::RecordNotObject(2),
                ContextJsonlIssue::InvalidField {
                    line: 3,
                    field: "id".to_owned(),
                    value: "bad/id".to_owned()
                },
                ContextJsonlIssue::MissingStringField {
                    line: 3,
                    field: "source".to_owned()
                }
            ]
        );

        let refs = inspect_context_jsonl(
            ContextJsonlKind::Refs,
            r#"{"id":"r1","path":"../secret","kind":"provider_thread","summary":"bad"}
"#,
        );
        assert_eq!(
            refs.issues(),
            [
                ContextJsonlIssue::InvalidField {
                    line: 1,
                    field: "path".to_owned(),
                    value: "../secret".to_owned()
                },
                ContextJsonlIssue::InvalidField {
                    line: 1,
                    field: "kind".to_owned(),
                    value: "provider_thread".to_owned()
                }
            ]
        );

        let dedup = inspect_context_jsonl(
            ContextJsonlKind::DedupIndex,
            r#"{"hash":"md5-old","refs":[],"bytes":"120","tokens":3000}
"#,
        );
        assert_eq!(
            dedup.issues(),
            [
                ContextJsonlIssue::InvalidField {
                    line: 1,
                    field: "hash".to_owned(),
                    value: "md5-old".to_owned()
                },
                ContextJsonlIssue::MissingStringArrayField {
                    line: 1,
                    field: "refs".to_owned()
                },
                ContextJsonlIssue::MissingNumberField {
                    line: 1,
                    field: "bytes".to_owned()
                }
            ]
        );
    }

    #[test]
    fn event_stream_accepts_canonical_model_jsonl() {
        let report = inspect_event_stream_jsonl(
            r#"{"type":"start","run":"r1","model":"qwen"}
{"type":"delta","run":"r1","text":"hello"}
{"type":"message","run":"r1","role":"assistant","content":[{"type":"text","text":"hello"}]}
{"type":"tool_call","run":"r1","id":"call-1","name":"fs.read","arguments":{"path":"README.md"}}
{"type":"usage","run":"r1","input_tokens":10,"output_tokens":1}
{"type":"done","run":"r1","status":"ok"}
"#,
        );
        assert!(report.is_ok());
        assert!(report.issues().is_empty());
    }

    #[test]
    fn event_stream_accepts_stable_error_frames() {
        let report = inspect_event_stream_jsonl(
            r#"{"type":"error","run":"r1","code":"EACCES","message":"permission denied"}
{"type":"done","run":"r1","status":"error"}
"#,
        );
        assert!(report.is_ok());
    }

    #[test]
    fn event_stream_accepts_child_lifecycle_frames() {
        let report = inspect_event_stream_jsonl(
            r#"{"type":"agent.child.cancel","parent":"coder","child":"rev-123","reason":"parent_dead"}
{"type":"agent.stop","agent":"rev-123","status":"cancelled"}
"#,
        );
        assert!(report.is_ok());
        assert!(report.issues().is_empty());
    }

    #[test]
    fn event_stream_rejects_provider_native_state_and_unknown_events() {
        let report = inspect_event_stream_jsonl(
            r#"{"type":"start","run":"r1","model":"qwen","response_id":"resp_123"}
{"type":"native_thread","run":"r1","thread_id":"thread_123"}
{"type":"message","run":"r1","content":[{"type":"text","text":"x","provider_response_id":"abc"}]}
"#,
        );
        assert_eq!(
            report.issues(),
            [
                EventStreamIssue::ProviderNativeField {
                    line: 1,
                    field: "response_id".to_owned()
                },
                EventStreamIssue::ProviderNativeField {
                    line: 2,
                    field: "thread_id".to_owned()
                },
                EventStreamIssue::UnknownType {
                    line: 2,
                    event_type: "native_thread".to_owned()
                },
                EventStreamIssue::ProviderNativeField {
                    line: 3,
                    field: "provider_response_id".to_owned()
                }
            ]
        );
    }

    #[test]
    fn event_stream_rejects_invalid_shape_and_specialized_frames() {
        let report = inspect_event_stream_jsonl(
            r#"not-json
[]
{"run":"r1"}
{"type":"delta","text":"missing run"}
{"type":"error","run":"r1","code":"PROVIDER_DENIED"}
{"type":"done","run":"r1","status":"maybe"}
{"type":"usage","run":"r1","input_tokens":"10","output_tokens":1}
{"type":"tool_call","run":"r1","id":"bad/id","name":"fs.read"}
{"type":"agent.child.cancel","parent":"bad/parent","child":"rev-1","reason":"manual"}
{"type":"agent.stop","agent":"rev-1","status":"dead"}
"#,
        );
        assert_eq!(
            report.issues(),
            [
                EventStreamIssue::InvalidJson(1),
                EventStreamIssue::EventNotObject(2),
                EventStreamIssue::MissingType(3),
                EventStreamIssue::MissingRun(4),
                EventStreamIssue::InvalidErrorCode(5),
                EventStreamIssue::InvalidDoneStatus(6),
                EventStreamIssue::InvalidUsage(7),
                EventStreamIssue::InvalidToolCall(8),
                EventStreamIssue::InvalidAgentLifecycle(9),
                EventStreamIssue::InvalidAgentLifecycle(10)
            ]
        );
    }

    #[test]
    fn shared_queue_layout_inspector_checks_recommended_dirs() {
        let root = unique_test_dir("shared-queue-layout");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        for dir in SHARED_QUEUE_REQUIRED_DIRS {
            assert!(fs::create_dir_all(root.join(dir)).is_ok());
        }
        let report = inspect_shared_queue_layout(&root);
        assert!(report.is_ok());

        assert!(fs::remove_dir_all(root.join("failed")).is_ok());
        assert!(fs::remove_dir_all(root.join("done")).is_ok());
        assert!(fs::write(root.join("done"), "not a dir\n").is_ok());
        let report = inspect_shared_queue_layout(&root);
        assert!(!report.is_ok());
        assert!(
            report
                .issues()
                .contains(&SharedQueueLayoutIssue::MissingDirectory(
                    "failed".to_owned()
                ))
        );
        assert!(
            report
                .issues()
                .contains(&SharedQueueLayoutIssue::NotDirectory("done".to_owned()))
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn shared_queue_claim_uses_atomic_claim_directories() {
        let root = unique_test_dir("shared-queue-claim");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_shared_queue_layout(&root);
        write_text_file(&root.join("pending").join("job-2.req.json"), "two\n");
        write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
        write_text_file(&root.join("pending").join(".ignored"), "bad\n");
        assert!(fs::create_dir_all(root.join("pending").join("not-file")).is_ok());

        let first = claim_next_shared_queue_job(&root, "worker-a");
        assert!(first.is_ok());
        let Ok(Some(first)) = first else { return };
        assert_eq!(first.job_name(), "job-1.req.json");
        let claimed_content = fs::read_to_string(first.claimed_path());
        assert!(matches!(claimed_content, Ok(ref content) if content == "one\n"));
        let lease_worker = fs::read_to_string(first.lease_path().join("worker"));
        assert!(matches!(lease_worker, Ok(ref content) if content == "worker-a\n"));
        assert!(!root.join("pending").join("job-1.req.json").exists());

        let second = claim_next_shared_queue_job(&root, "worker-b");
        assert!(second.is_ok());
        let Ok(Some(second)) = second else { return };
        assert_eq!(second.job_name(), "job-2.req.json");

        let none = claim_next_shared_queue_job(&root, "worker-c");
        assert_eq!(none, Ok(None));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn shared_queue_claim_skips_existing_claim_lock() {
        let root = unique_test_dir("shared-queue-claim-lock");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_shared_queue_layout(&root);
        write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");
        write_text_file(&root.join("pending").join("job-2.req.json"), "two\n");
        assert!(fs::create_dir_all(root.join("claimed").join("job-1.req.json")).is_ok());

        let claimed = claim_next_shared_queue_job(&root, "worker-a");
        assert!(claimed.is_ok());
        let Ok(Some(claimed)) = claimed else { return };
        assert_eq!(claimed.job_name(), "job-2.req.json");
        assert!(root.join("pending").join("job-1.req.json").exists());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn shared_queue_recovery_requeues_claimed_job_with_lease() {
        let root = unique_test_dir("shared-queue-recover");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_shared_queue_layout(&root);
        write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");

        let claimed = claim_next_shared_queue_job(&root, "worker-a");
        assert!(claimed.is_ok());
        let Ok(Some(claimed)) = claimed else { return };
        assert!(claimed.claimed_path().is_file());
        assert!(claimed.lease_path().join("worker").is_file());

        let recovered = recover_shared_queue_job(&root, "job-1.req.json");
        assert_eq!(recovered, Ok(root.join("pending").join("job-1.req.json")));
        let recovered_content = fs::read_to_string(root.join("pending").join("job-1.req.json"));
        assert!(matches!(recovered_content, Ok(ref content) if content == "one\n"));
        assert!(!root.join("claimed").join("job-1.req.json").exists());
        assert!(!root.join("lease").join("job-1.req.json").exists());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn shared_queue_recovery_requires_existing_claim_and_lease() {
        let root = unique_test_dir("shared-queue-recover-missing");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_shared_queue_layout(&root);
        assert_eq!(
            recover_shared_queue_job(&root, "job-1.req.json"),
            Err(SharedQueueRecoverError::MissingClaim)
        );

        let claim_dir = root.join("claimed").join("job-1.req.json");
        assert!(fs::create_dir_all(&claim_dir).is_ok());
        write_text_file(&claim_dir.join("job-1.req.json"), "one\n");
        assert_eq!(
            recover_shared_queue_job(&root, "job-1.req.json"),
            Err(SharedQueueRecoverError::MissingLease)
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn shared_queue_finish_writes_readable_done_result_and_cleans_lease() {
        let root = unique_test_dir("shared-queue-finish-done");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_shared_queue_layout(&root);
        write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");

        let claimed = claim_next_shared_queue_job(&root, "worker-a");
        assert!(claimed.is_ok());
        let Ok(Some(claimed)) = claimed else { return };
        let result_path =
            finish_shared_queue_job(&root, claimed.job_name(), SharedQueueOutcome::Done, b"ok\n");
        assert_eq!(
            result_path,
            Ok(root.join("done").join("job-1.req.json.result"))
        );
        let result = fs::read_to_string(root.join("done").join("job-1.req.json.result"));
        assert!(matches!(result, Ok(ref content) if content == "ok\n"));
        let request = fs::read_to_string(root.join("done").join("job-1.req.json"));
        assert!(matches!(request, Ok(ref content) if content == "one\n"));
        assert!(!root.join("claimed").join("job-1.req.json").exists());
        assert!(!root.join("lease").join("job-1.req.json").exists());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn shared_queue_finish_writes_readable_failed_result() {
        let root = unique_test_dir("shared-queue-finish-failed");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        create_shared_queue_layout(&root);
        write_text_file(&root.join("pending").join("job-1.req.json"), "one\n");

        let claimed = claim_next_shared_queue_job(&root, "worker-a");
        assert!(claimed.is_ok());
        let Ok(Some(claimed)) = claimed else { return };
        let result_path = finish_shared_queue_job(
            &root,
            claimed.job_name(),
            SharedQueueOutcome::Failed,
            b"err\n",
        );
        assert_eq!(
            result_path,
            Ok(root.join("failed").join("job-1.req.json.result"))
        );
        let result = fs::read_to_string(root.join("failed").join("job-1.req.json.result"));
        assert!(matches!(result, Ok(ref content) if content == "err\n"));
        let request = fs::read_to_string(root.join("failed").join("job-1.req.json"));
        assert!(matches!(request, Ok(ref content) if content == "one\n"));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn shared_access_authority_requires_mount_linux_permission_and_policy() {
        let root = unique_test_dir("shared-authority-ok");
        let shared = root.join("shared-project-a");
        let file = shared.join("data.txt");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(&shared).is_ok());
        write_fixture_file(&file, 0o400);

        let metadata = fs::metadata(&file);
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let mounts = mount_table_for_source_target(
            "/ctx/shared/project-a",
            &shared,
            "ro",
            "bind,nosuid,nodev,noexec",
        );
        let policy = allow_shared_policy("coder_t", "project-a", SharedAccess::Read);
        let authority = SharedAccessAuthority::new(&identity, &mounts, "coder_t", &policy);

        assert_eq!(
            authorize_shared_access("project-a", &file, SharedAccess::Read, authority),
            Ok(())
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn shared_access_authority_denies_write_on_read_only_mount() {
        let root = unique_test_dir("shared-authority-ro");
        let shared = root.join("shared-project-a");
        let file = shared.join("data.txt");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(&shared).is_ok());
        write_fixture_file(&file, 0o600);

        let metadata = fs::metadata(&file);
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let mounts = mount_table_for_source_target(
            "/ctx/shared/project-a",
            &shared,
            "ro",
            "bind,nosuid,nodev",
        );
        let policy = allow_shared_policy("coder_t", "project-a", SharedAccess::Write);
        let authority = SharedAccessAuthority::new(&identity, &mounts, "coder_t", &policy);

        assert_eq!(
            authorize_shared_access("project-a", &file, SharedAccess::Write, authority),
            Err(SharedAccessDenial::ReadOnlyMount)
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn shared_access_authority_denies_missing_policy_and_wrong_space() {
        let root = unique_test_dir("shared-authority-policy");
        let shared = root.join("shared-project-a");
        let file = shared.join("data.txt");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(&shared).is_ok());
        write_fixture_file(&file, 0o400);

        let metadata = fs::metadata(&file);
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let mounts = mount_table_for_source_target(
            "/ctx/shared/project-a",
            &shared,
            "ro",
            "bind,nosuid,nodev",
        );
        let wrong_mounts = mount_table_for_source_target(
            "/ctx/shared/project-b",
            &shared,
            "ro",
            "bind,nosuid,nodev",
        );
        let empty_policy = PolicyV0::parse("");
        assert!(empty_policy.is_ok());
        let Ok(empty_policy) = empty_policy else {
            return;
        };
        let policy = allow_shared_policy("coder_t", "project-a", SharedAccess::Read);

        assert_eq!(
            authorize_shared_access(
                "project-a",
                &file,
                SharedAccess::Read,
                SharedAccessAuthority::new(&identity, &mounts, "coder_t", &empty_policy),
            ),
            Err(SharedAccessDenial::Policy)
        );
        assert_eq!(
            authorize_shared_access(
                "project-a",
                &file,
                SharedAccess::Read,
                SharedAccessAuthority::new(&identity, &wrong_mounts, "coder_t", &policy),
            ),
            Err(SharedAccessDenial::WrongSharedPath)
        );
        assert_eq!(
            authorize_shared_access(
                "project-a",
                &file,
                SharedAccess::Read,
                SharedAccessAuthority::new(&identity, &MountTable::default(), "coder_t", &policy,),
            ),
            Err(SharedAccessDenial::NotMounted)
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn shared_access_authority_checks_linux_mode_bits() {
        let root = unique_test_dir("shared-authority-linux");
        let shared = root.join("shared-project-a");
        let file = shared.join("data.txt");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(&shared).is_ok());
        write_fixture_file(&file, 0o400);

        let metadata = fs::metadata(&file);
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let other_identity = AgentUnixIdentity::new(
            metadata.uid().saturating_add(1),
            metadata.gid().saturating_add(1),
            [],
        );
        let mounts = mount_table_for_source_target(
            "/ctx/shared/project-a",
            &shared,
            "ro",
            "bind,nosuid,nodev",
        );
        let policy = allow_shared_policy("coder_t", "project-a", SharedAccess::Read);
        let authority = SharedAccessAuthority::new(&other_identity, &mounts, "coder_t", &policy);

        assert_eq!(
            authorize_shared_access("project-a", &file, SharedAccess::Read, authority),
            Err(SharedAccessDenial::LinuxPermission)
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn session_access_authority_allows_explicit_im_channel_session() {
        let root = unique_test_dir("session-authority-im-ok");
        let shared = root.join("im-qq-dev");
        let messages = shared
            .join("agent")
            .join("bot")
            .join("session")
            .join("group-456")
            .join("messages.jsonl");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        write_fixture_file(&messages, 0o600);

        let metadata = fs::metadata(&messages);
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let mounts = mount_table_for_source_target(
            "/ctx/shared/im-qq-dev",
            &shared,
            "ro",
            "bind,nosuid,nodev,noexec",
        );
        let policy = policy_with_rules([
            "allow bot_t shared:im-qq-dev read",
            "allow bot_t session:group-456 read",
        ]);
        let authority = SessionAccessAuthority::new(&identity, &mounts, "bot_t", &policy);

        assert_eq!(
            authorize_session_access(&messages, SessionAccess::Read, authority),
            Ok(())
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn session_access_authority_denies_cross_channel_without_session_policy() {
        let root = unique_test_dir("session-authority-im-deny");
        let shared = root.join("im-qq-dev");
        let allowed = shared
            .join("agent")
            .join("bot")
            .join("session")
            .join("group-456")
            .join("messages.jsonl");
        let other = shared
            .join("agent")
            .join("bot")
            .join("session")
            .join("group-999")
            .join("messages.jsonl");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        write_fixture_file(&allowed, 0o600);
        write_fixture_file(&other, 0o600);

        let metadata = fs::metadata(&allowed);
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let mounts = mount_table_for_source_target(
            "/ctx/shared/im-qq-dev",
            &shared,
            "ro",
            "bind,nosuid,nodev,noexec",
        );
        let policy = policy_with_rules([
            "allow bot_t shared:im-qq-dev read",
            "allow bot_t session:group-456 read",
        ]);
        let authority = SessionAccessAuthority::new(&identity, &mounts, "bot_t", &policy);

        assert_eq!(
            authorize_session_access(&allowed, SessionAccess::Read, authority),
            Ok(())
        );
        assert_eq!(
            authorize_session_access(&other, SessionAccess::Read, authority),
            Err(SessionAccessDenial::SessionPolicy)
        );
        assert_eq!(SessionAccessDenial::SessionPolicy.errno(), "EACCES");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn session_access_authority_requires_shared_policy_and_mount_write_mode() {
        let root = unique_test_dir("session-authority-shared-policy");
        let shared = root.join("im-slack-company");
        let messages = shared
            .join("agent")
            .join("bot")
            .join("session")
            .join("channel-789")
            .join("messages.jsonl");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        write_fixture_file(&messages, 0o600);

        let metadata = fs::metadata(&messages);
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let ro_mounts = mount_table_for_source_target(
            "/ctx/shared/im-slack-company",
            &shared,
            "ro",
            "bind,nosuid,nodev,noexec",
        );
        let writable_mounts = mount_table_for_source_target(
            "/ctx/shared/im-slack-company",
            &shared,
            "rw",
            "bind,nosuid,nodev",
        );
        let session_only = policy_with_rules(["allow bot_t session:channel-789 read"]);
        let read_policy = policy_with_rules([
            "allow bot_t shared:im-slack-company read",
            "allow bot_t session:channel-789 write",
        ]);

        assert_eq!(
            authorize_session_access(
                &messages,
                SessionAccess::Read,
                SessionAccessAuthority::new(&identity, &ro_mounts, "bot_t", &session_only),
            ),
            Err(SessionAccessDenial::SharedPolicy)
        );
        assert_eq!(
            authorize_session_access(
                &messages,
                SessionAccess::Write,
                SessionAccessAuthority::new(&identity, &ro_mounts, "bot_t", &read_policy),
            ),
            Err(SessionAccessDenial::ReadOnlyMount)
        );
        assert_eq!(
            authorize_session_access(
                &messages,
                SessionAccess::Write,
                SessionAccessAuthority::new(&identity, &writable_mounts, "bot_t", &read_policy),
            ),
            Err(SessionAccessDenial::SharedPolicy)
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn session_access_authority_enforces_private_home_uid() {
        let root = unique_test_dir("session-authority-private-uid");
        let home = root.join("home-1000");
        let messages = home
            .join("agent")
            .join("coder")
            .join("session")
            .join("default")
            .join("messages.jsonl");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        write_fixture_file(&messages, 0o644);

        let metadata = fs::metadata(&messages);
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let owner_identity = AgentUnixIdentity::new(1000, metadata.gid(), []);
        let other_identity = AgentUnixIdentity::new(1001, metadata.gid(), []);
        let mounts = mount_table_for_source_target(
            "/ctx/home/1000",
            &home,
            "ro",
            "bind,nosuid,nodev,noexec",
        );
        let policy = policy_with_rules(["allow coder_t session:default read"]);

        assert_eq!(
            authorize_session_access(
                &messages,
                SessionAccess::Read,
                SessionAccessAuthority::new(&owner_identity, &mounts, "coder_t", &policy),
            ),
            Ok(())
        );
        assert_eq!(
            authorize_session_access(
                &messages,
                SessionAccess::Read,
                SessionAccessAuthority::new(&other_identity, &mounts, "coder_t", &policy),
            ),
            Err(SessionAccessDenial::LinuxPermission)
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn session_access_authority_rejects_unmounted_and_non_session_paths() {
        let root = unique_test_dir("session-authority-path-shape");
        let shared = root.join("project-a");
        let file = shared.join("data").join("note.txt");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        write_fixture_file(&file, 0o644);

        let metadata = fs::metadata(&file);
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let mounts = mount_table_for_source_target(
            "/ctx/shared/project-a",
            &shared,
            "ro",
            "bind,nosuid,nodev,noexec",
        );
        let policy = policy_with_rules([
            "allow coder_t shared:project-a read",
            "allow coder_t session:default read",
        ]);

        assert_eq!(
            authorize_session_access(
                &file,
                SessionAccess::Read,
                SessionAccessAuthority::new(&identity, &mounts, "coder_t", &policy),
            ),
            Err(SessionAccessDenial::InvalidSessionPath)
        );
        assert_eq!(
            authorize_session_access(
                &file,
                SessionAccess::Read,
                SessionAccessAuthority::new(&identity, &MountTable::default(), "coder_t", &policy),
            ),
            Err(SessionAccessDenial::NotMounted)
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn ctx_path_parses_without_implicit_current_directory() {
        let path = ToolPath::parse(":/ctx/tool::/ctx/home/1000/tool:");
        assert_eq!(
            path.dirs(),
            [
                PathBuf::from("/ctx/tool"),
                PathBuf::from("/ctx/home/1000/tool")
            ]
        );
    }

    #[test]
    fn tool_lookup_uses_first_executable_hit() {
        let root = unique_test_dir("tool-lookup");
        let global = root.join("global-tool");
        let user = root.join("user-tool");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(&global).is_ok());
        assert!(fs::create_dir_all(&user).is_ok());

        write_fixture_file(&global.join("fs.read"), 0o644);
        write_fixture_file(&global.join("fs.write"), 0o755);
        write_fixture_file(&user.join("fs.read"), 0o755);
        assert!(fs::create_dir_all(user.join("fs.read.d")).is_ok());

        let path = ToolPath::new([global.clone(), user.clone()]);
        let found = path.find("fs.read");
        assert!(matches!(found, Ok(Some(ref hit)) if hit.path() == user.join("fs.read")));
        assert!(matches!(found, Ok(Some(ref hit)) if hit.control_dir() == user.join("fs.read.d")));

        write_fixture_file(&global.join("fs.read"), 0o755);
        let found = path.find("fs.read");
        assert!(matches!(found, Ok(Some(ref hit)) if hit.path() == global.join("fs.read")));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_listing_ignores_non_executable_and_control_entries() {
        let root = unique_test_dir("tool-list");
        let tools = root.join("tool");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
        write_fixture_file(&tools.join("fs.read"), 0o755);
        write_fixture_file(&tools.join("not.exec"), 0o644);
        write_fixture_file(&tools.join("bad.sock"), 0o755);

        let hits = ToolPath::new([tools.clone()]).list();
        assert!(hits.is_ok());
        let Ok(hits) = hits else { return };
        let expected = tools.join("fs.read");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits.first().map(ToolHit::path), Some(expected.as_path()));

        let invalid = ToolPath::new([tools]).find("../bad");
        assert_eq!(invalid, Err(ToolPathError::InvalidName));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_execution_authority_requires_all_layers() {
        let root = unique_test_dir("tool-authority-ok");
        let tools = root.join("tool");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
        write_fixture_file(&tools.join("fs.read"), 0o755);

        let metadata = fs::metadata(tools.join("fs.read"));
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
        let agent_policy = allow_tool_policy("coder_t", "fs.read");
        let tool_policy = allow_tool_policy("coder_t", "fs.read");
        let tool_path = ToolPath::new([tools.clone()]);
        let authority =
            ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &agent_policy, &tool_policy);

        let grant = authorize_tool_execution(&tool_path, "fs.read", authority);
        assert!(matches!(grant, Ok(ref grant) if grant.hit().path() == tools.join("fs.read")));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn model_tool_call_syntax_does_not_execute_tools() {
        let root = unique_test_dir("tool-authority-model-boundary");
        let tools = root.join("tool");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
        write_fixture_file(&tools.join("fs.read"), 0o755);

        let model_event = inspect_event_stream_jsonl(
            r#"{"type":"tool_call","run":"r1","id":"call-1","name":"fs.read","arguments":{"path":"README.md"}}
"#,
        );
        assert!(model_event.is_ok());

        let metadata = fs::metadata(tools.join("fs.read"));
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
        let policy = allow_tool_policy("qwen_t", "fs.read");
        let tool_path = ToolPath::new([tools]);
        assert_ne!(ToolExecutionPrincipal::Model, ToolExecutionPrincipal::Agent);

        let denied = authorize_tool_execution(
            &tool_path,
            "fs.read",
            ToolExecutionAuthority::model(&identity, &mounts, "qwen_t", &policy, &policy),
        );
        assert_eq!(denied, Err(ToolExecutionDenial::ModelCannotExecute));
        assert_eq!(ToolExecutionDenial::ModelCannotExecute.errno(), "EACCES");

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn prompt_skill_and_mcp_config_cannot_grant_tool_execution() {
        let root = unique_test_dir("tool-authority-text-no-grant");
        let tools = root.join("tool");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
        write_fixture_file(&tools.join("fs.read"), 0o755);
        write_text_file(
            &root
                .join("session")
                .join("context")
                .join("pinned")
                .join("system.md"),
            "allow coder_t tool:fs.read execute\n",
        );
        write_text_file(
            &root.join("work").join("AGENTS.md"),
            "The agent may use fs.read for this task.\n",
        );
        write_text_file(
            &root.join("work").join(".mcp.json"),
            "{\"servers\":{\"fs\":{\"allow\":\"fs.read\"}}}\n",
        );
        assert!(root.join("work").join("AGENTS.md").is_file());
        assert!(root.join("work").join(".mcp.json").is_file());

        let metadata = fs::metadata(tools.join("fs.read"));
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
        let empty_policy = PolicyV0::parse("");
        assert!(empty_policy.is_ok());
        let Ok(empty_policy) = empty_policy else {
            return;
        };
        let tool_policy = allow_tool_policy("coder_t", "fs.read");
        let tool_path = ToolPath::new([tools]);

        let denied = authorize_tool_execution(
            &tool_path,
            "fs.read",
            ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &empty_policy, &tool_policy),
        );
        assert_eq!(denied, Err(ToolExecutionDenial::AgentPolicy));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_execution_authority_denies_without_policy_or_mount_exec() {
        let root = unique_test_dir("tool-authority-deny");
        let tools = root.join("tool");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
        write_fixture_file(&tools.join("fs.read"), 0o755);
        write_text_file(
            &tools.join("fs.read.d").join("schema"),
            "{\"type\":\"object\"}\n",
        );

        let metadata = fs::metadata(tools.join("fs.read"));
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let executable_mount = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
        let noexec_mount = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev,noexec");
        let agent_policy = allow_tool_policy("coder_t", "fs.read");
        let tool_policy = allow_tool_policy("coder_t", "fs.read");
        let empty_policy = PolicyV0::parse("");
        assert!(empty_policy.is_ok());
        let Ok(empty_policy) = empty_policy else {
            return;
        };
        let tool_path = ToolPath::new([tools]);

        let denied_by_noexec = authorize_tool_execution(
            &tool_path,
            "fs.read",
            ToolExecutionAuthority::new(
                &identity,
                &noexec_mount,
                "coder_t",
                &agent_policy,
                &tool_policy,
            ),
        );
        assert_eq!(denied_by_noexec, Err(ToolExecutionDenial::NoExecMount));

        let denied_by_agent_policy = authorize_tool_execution(
            &tool_path,
            "fs.read",
            ToolExecutionAuthority::new(
                &identity,
                &executable_mount,
                "coder_t",
                &empty_policy,
                &tool_policy,
            ),
        );
        assert_eq!(
            denied_by_agent_policy,
            Err(ToolExecutionDenial::AgentPolicy)
        );

        let denied_by_tool_policy = authorize_tool_execution(
            &tool_path,
            "fs.read",
            ToolExecutionAuthority::new(
                &identity,
                &executable_mount,
                "coder_t",
                &agent_policy,
                &empty_policy,
            ),
        );
        assert_eq!(denied_by_tool_policy, Err(ToolExecutionDenial::ToolPolicy));

        let denied_when_unmounted = authorize_tool_execution(
            &tool_path,
            "fs.read",
            ToolExecutionAuthority::new(
                &identity,
                &MountTable::default(),
                "coder_t",
                &agent_policy,
                &tool_policy,
            ),
        );
        assert_eq!(denied_when_unmounted, Err(ToolExecutionDenial::NotMounted));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn project_tools_are_visible_only_through_ctx_path_order() {
        let root = unique_test_dir("tool-authority-project-path");
        let global = root.join("ctx-tool");
        let project = root.join("shared-project-tool");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(global.join("project.test.d")).is_ok());
        assert!(fs::create_dir_all(project.join("project.test.d")).is_ok());
        write_fixture_file(&global.join("project.test"), 0o644);
        write_fixture_file(&project.join("project.test"), 0o755);

        assert_eq!(
            ToolPath::new([global.clone()]).find("project.test"),
            Ok(None)
        );
        let with_project = ToolPath::new([global, project.clone()]);
        let found = with_project.find("project.test");
        assert!(matches!(found, Ok(Some(ref hit)) if hit.path() == project.join("project.test")));

        let metadata = fs::metadata(project.join("project.test"));
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let mounts = mount_table_for_target(&project, "rw", "bind,nosuid,nodev");
        let policy = allow_tool_policy("coder_t", "project.test");
        let authority =
            ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &policy, &policy);
        assert!(authorize_tool_execution(&with_project, "project.test", authority).is_ok());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn mcp_backed_tool_is_ordinary_tool_and_still_requires_policy() {
        let root = unique_test_dir("tool-authority-mcp");
        let tools = root.join("tool");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(tools.join("mcp.github.search_issues.d")).is_ok());
        write_fixture_file(&tools.join("mcp.github.search_issues"), 0o755);
        write_text_file(
            &tools.join("mcp.github.search_issues.d").join("schema"),
            "{\"type\":\"object\"}\n",
        );
        write_text_file(
            &root.join("work").join(".mcp.json"),
            "{\"servers\":{\"github\":{}}}\n",
        );

        let metadata = fs::metadata(tools.join("mcp.github.search_issues"));
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
        let tool_path = ToolPath::new([tools]);
        let empty_policy = PolicyV0::parse("");
        assert!(empty_policy.is_ok());
        let Ok(empty_policy) = empty_policy else {
            return;
        };
        let allow_mcp = allow_tool_policy("coder_t", "mcp.github.search_issues");

        let denied = authorize_tool_execution(
            &tool_path,
            "mcp.github.search_issues",
            ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &empty_policy, &allow_mcp),
        );
        assert_eq!(denied, Err(ToolExecutionDenial::AgentPolicy));

        let allowed = authorize_tool_execution(
            &tool_path,
            "mcp.github.search_issues",
            ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &allow_mcp, &allow_mcp),
        );
        assert!(allowed.is_ok());

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_schema_cannot_grant_execution_authority() {
        let root = unique_test_dir("tool-authority-schema-no-grant");
        let tools = root.join("tool");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(tools.join("fs.read.d")).is_ok());
        write_fixture_file(&tools.join("fs.read"), 0o755);
        write_text_file(
            &tools.join("fs.read.d").join("schema"),
            "{\"policy\":\"allow coder_t tool:fs.read execute\"}\n",
        );

        let metadata = fs::metadata(tools.join("fs.read"));
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
        let tool_path = ToolPath::new([tools]);
        let empty_policy = PolicyV0::parse("");
        assert!(empty_policy.is_ok());
        let Ok(empty_policy) = empty_policy else {
            return;
        };
        let tool_policy = allow_tool_policy("coder_t", "fs.read");

        let denied = authorize_tool_execution(
            &tool_path,
            "fs.read",
            ToolExecutionAuthority::new(&identity, &mounts, "coder_t", &empty_policy, &tool_policy),
        );
        assert_eq!(denied, Err(ToolExecutionDenial::AgentPolicy));

        let _ignored = fs::remove_dir_all(&root);
    }

    #[test]
    fn tool_execution_authority_checks_linux_identity_mode_bits() {
        let root = unique_test_dir("tool-authority-linux");
        let tools = root.join("tool");
        assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
        assert!(fs::create_dir_all(&tools).is_ok());
        write_fixture_file(&tools.join("owner-only"), 0o100);

        let metadata = fs::metadata(tools.join("owner-only"));
        assert!(metadata.is_ok());
        let Ok(metadata) = metadata else { return };
        let owner_identity = AgentUnixIdentity::new(metadata.uid(), metadata.gid(), []);
        let other_identity = AgentUnixIdentity::new(
            metadata.uid().saturating_add(1),
            metadata.gid().saturating_add(1),
            [],
        );
        let mounts = mount_table_for_target(&tools, "rw", "bind,nosuid,nodev");
        let policy = allow_tool_policy("coder_t", "owner-only");
        let tool_path = ToolPath::new([tools]);

        assert!(
            authorize_tool_execution(
                &tool_path,
                "owner-only",
                ToolExecutionAuthority::new(&owner_identity, &mounts, "coder_t", &policy, &policy),
            )
            .is_ok()
        );
        assert_eq!(
            authorize_tool_execution(
                &tool_path,
                "owner-only",
                ToolExecutionAuthority::new(&other_identity, &mounts, "coder_t", &policy, &policy),
            ),
            Err(ToolExecutionDenial::LinuxPermission)
        );

        let _ignored = fs::remove_dir_all(&root);
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!("cortexfs-{name}-{}-{nanos}", std::process::id()))
    }

    fn write_fixture_file(path: &Path, mode: u32) {
        if let Some(parent) = path.parent() {
            assert!(fs::create_dir_all(parent).is_ok());
        }
        assert!(fs::write(path, "#!/bin/sh\n").is_ok());
        let permissions = fs::metadata(path).map(|metadata| metadata.permissions());
        assert!(permissions.is_ok());
        let Ok(mut permissions) = permissions else {
            return;
        };
        permissions.set_mode(mode);
        assert!(fs::set_permissions(path, permissions).is_ok());
    }

    fn create_complete_session_layout(session: &Path) {
        let context = session.join("context");
        assert!(fs::create_dir_all(context.join("pinned")).is_ok());
        assert!(fs::create_dir_all(context.join("swap")).is_ok());
        assert!(fs::create_dir_all(context.join("dedup")).is_ok());
        assert!(fs::create_dir_all(context.join("child").join("rev-1").join("artifact")).is_ok());

        for file in SESSION_REQUIRED_FILES {
            write_text_file(&session.join(file), session_file_fixture_value(file));
        }
        for file in super::CONTEXT_REQUIRED_FILES {
            write_text_file(&context.join(file), "ok\n");
        }
        for file in super::CHILD_RESULT_REQUIRED_FILES {
            write_text_file(&context.join("child").join("rev-1").join(file), "ok\n");
        }
    }

    fn session_file_fixture_value(file: &str) -> &'static str {
        match file {
            "state" => "idle\n",
            "cwd" => "/work\n",
            "meta.json" => "{\"client\":\"ctx\",\"model\":\"qwen\",\"scope\":\"private\"}\n",
            _ => "ok\n",
        }
    }

    fn write_text_file(path: &Path, content: &str) {
        let Some(parent) = path.parent() else {
            return;
        };
        assert!(fs::create_dir_all(parent).is_ok());
        assert!(fs::write(path, content).is_ok());
    }

    fn create_shared_queue_layout(queue: &Path) {
        for dir in SHARED_QUEUE_REQUIRED_DIRS {
            assert!(fs::create_dir_all(queue.join(dir)).is_ok());
        }
    }

    fn mount_table_for_target(target: &Path, mode: &str, options: &str) -> MountTable {
        mount_table_for_source_target(&target.display().to_string(), target, mode, options)
    }

    fn mount_table_for_source_target(
        source: &str,
        target: &Path,
        mode: &str,
        options: &str,
    ) -> MountTable {
        let line = format!(
            "{source}\t{target}\t{mode}\t{options}\n",
            target = target.display()
        );
        let parsed = MountTable::parse(&line);
        assert!(parsed.is_ok());
        parsed.unwrap_or_default()
    }

    fn allow_tool_policy(subject: &str, tool: &str) -> PolicyV0 {
        let parsed = PolicyV0::parse(&format!("allow {subject} tool:{tool} execute\n"));
        assert!(parsed.is_ok());
        parsed.unwrap_or_default()
    }

    fn allow_shared_policy(subject: &str, shared: &str, access: SharedAccess) -> PolicyV0 {
        let permission = match access {
            SharedAccess::Read => "read",
            SharedAccess::Write => "write",
        };
        let parsed = PolicyV0::parse(&format!("allow {subject} shared:{shared} {permission}\n"));
        assert!(parsed.is_ok());
        parsed.unwrap_or_default()
    }

    fn policy_with_rules(rules: impl IntoIterator<Item = &'static str>) -> PolicyV0 {
        let content = rules.into_iter().collect::<Vec<_>>().join("\n") + "\n";
        let parsed = PolicyV0::parse(&content);
        assert!(parsed.is_ok());
        parsed.unwrap_or_default()
    }

    fn env_value<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
        env.iter()
            .find_map(|entry| (entry.0 == key).then_some(entry.1.as_str()))
    }

    fn create_complete_object_layout(
        root: &Path,
        class: ObjectClass,
        name: &str,
        model_session: &str,
    ) {
        let class_dir = root.join(class.as_str());
        assert!(fs::create_dir_all(&class_dir).is_ok());
        write_fixture_file(&class_dir.join(name), 0o755);
        let control_dir = class_dir.join(format!("{name}.d"));
        assert!(fs::create_dir_all(&control_dir).is_ok());
        for file in object_control_files(class) {
            let value = if class == ObjectClass::Model && *file == "session" {
                model_session
            } else if class == ObjectClass::Model && *file == "cap" {
                "chat"
            } else if class == ObjectClass::Tool && *file == "schema" {
                "{\"type\":\"object\"}"
            } else if class == ObjectClass::Agent {
                agent_control_fixture_value(file)
            } else {
                "ok"
            };
            write_text_file(&control_dir.join(file), &format!("{value}\n"));
        }
    }

    fn agent_control_fixture_value(file: &str) -> &'static str {
        match file {
            "owner" | "uid" => "1000",
            "gid" => "100",
            "groups" => "10\n20",
            "label" => "user_u:agent_r:coder_t:s0",
            "iso" => "shared",
            "parent" | "pid" => "",
            "life" => "owned",
            "root" => "/ctx/home/1000/agent/coder/root",
            "cwd" => "/work",
            "env" => "CTX_ROOT=/ctx",
            "path" => "/ctx/tool:/ctx/home/1000/tool",
            "mount" => "/ctx\t/ctx\tro\trbind,nosuid,nodev",
            "model" => "qwen",
            "policy" => "allow coder_t model:qwen use",
            "status" => "idle",
            "log" => "agent/coder/log",
            "meta.json" => "{}",
            _ => "ok",
        }
    }

    fn object_control_files(class: ObjectClass) -> &'static [&'static str] {
        match class {
            ObjectClass::Model => MODEL_CONTROL_FILES,
            ObjectClass::Agent => AGENT_CONTROL_FILES,
            ObjectClass::Tool => TOOL_CONTROL_FILES,
        }
    }

    fn bind_socket(path: &Path) -> Option<UnixListener> {
        let parent = path.parent()?;
        assert!(fs::create_dir_all(parent).is_ok());
        UnixListener::bind(path).ok()
    }
}
