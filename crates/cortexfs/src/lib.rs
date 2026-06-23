//! `CortexFS` Agent OS ABI design core.
//!
//! The old CLI, daemon, provider registry, and FUSE projection were removed
//! before the Agent OS rewrite. This crate intentionally exposes only stable
//! ABI names while the implementation is redesigned around Rig.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
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

/// Default `CortexFS` mount root.
pub const CTX_ROOT: &str = "/ctx";

/// Rust object runner used by executable object metadata files.
pub const CORTEXFS_OBJECT_RUNNER: &str = "/usr/bin/cortexfs-object-runner";

/// Root entries reserved by the new Agent OS ABI.
pub const ROOT_ENTRIES: &[&str] = &["status", "bin", "model", "agent", "tool", "home", "shared"];

/// Object classes exposed as executable files.
pub const EXEC_OBJECTS: &[&str] = &["model", "agent", "tool"];

/// Maximum object name length.
pub const MAX_OBJECT_NAME_LEN: usize = 64;

/// Required model control files.
pub const MODEL_CONTROL_FILES: &[&str] =
    &["id", "driver", "cap", "default", "session", "status", "log"];

const DEBUG_ECHO_MODEL: &str = "debug/echo";
const DEBUG_ECHO_PROVIDER: &str = "debug";
const DEBUG_ECHO_NAME: &str = "echo";
const DEFAULT_MODEL_ALIAS: &str = "main";
const HELPER_MODEL_ALIAS: &str = "helper";
const DEFAULT_MODEL_ALIAS_TARGET: &str = "/ctx/model/debug/echo";
const SYSTEM_PROVIDER_CONFIG_DIR: &str = "/etc/cortexfs/providers.d";

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
    provider_config_dir: PathBuf,
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
    /// Schema is an object but not a valid JSON Schema document.
    InvalidSchema,
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

/// Result of inspecting `model/<provider>/<model>.d/cap`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelCapabilityReport {
    issues: Vec<ModelCapabilityIssue>,
}

/// Queryable model capability flag.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Capability {
    /// Model can consume image input.
    Vision,
    /// Model can emit tool-call syntax.
    Tools,
    /// Model supports JSON-mode or structured JSON output.
    JsonMode,
    /// Model can consume image input.
    ImageInput,
    /// Model can produce image output.
    ImageOutput,
    /// Model can consume audio input.
    AudioInput,
    /// Model can produce audio output.
    AudioOutput,
}

/// Provider-neutral model capability declaration.
#[expect(
    clippy::struct_excessive_bools,
    reason = "capability files expose independent stable boolean flags"
)]
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelCapabilities {
    pub context_length: usize,
    pub vision: bool,
    pub tools: bool,
    pub json_mode: bool,
    pub image_input: bool,
    pub image_output: bool,
    pub audio_input: bool,
    pub audio_output: bool,
}

/// Provider-neutral model capability lookup table.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelCapabilityRegistry {
    models: HashMap<String, ModelCapabilities>,
}

/// Error while reading or writing model capability registry data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModelRegistryError {
    /// Registry JSON could not be parsed.
    InvalidJson,
    /// Registry JSON has an unexpected shape.
    InvalidShape,
    /// Registry cache could not be read.
    CannotRead,
    /// Registry cache could not be written.
    CannotWrite,
}

/// Model driver call site used to select a driver route.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ModelDriverUseCase {
    /// Fallback route when no use-case-specific route is available.
    Default,
    /// One-shot execution through `model/<provider>/<model>`.
    Exec,
    /// Stateful model socket traffic through `model/<provider>/<model>.sock`.
    Socket,
    /// Agent-owned model calls.
    Agent,
}

/// Error while parsing `model/<provider>/<model>.d/driver`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ModelDriverRouteError {
    /// The route table has no usable driver declarations.
    Empty,
    /// A route-table line is missing `=`.
    MissingEquals { line: usize },
    /// A route-table key is not one of default, exec, socket, or agent.
    UnknownUseCase { line: usize, value: String },
    /// A route-table key appears more than once.
    DuplicateUseCase { line: usize, value: String },
    /// A driver list is empty or has an empty comma element.
    EmptyDriver { line: usize },
    /// A driver name is not a valid stable component.
    InvalidDriverName { line: usize, value: String },
}

/// Parsed `driver` control-file route table.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModelDriverRoutingTable {
    routes: HashMap<ModelDriverUseCase, Vec<String>>,
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

impl ModelCapabilities {
    /// Returns whether this declaration supports a capability.
    #[must_use]
    pub const fn supports(&self, capability: Capability) -> bool {
        match capability {
            Capability::Vision => self.vision,
            Capability::Tools => self.tools,
            Capability::JsonMode => self.json_mode,
            Capability::ImageInput => self.image_input,
            Capability::ImageOutput => self.image_output,
            Capability::AudioInput => self.audio_input,
            Capability::AudioOutput => self.audio_output,
        }
    }
}

impl ModelCapabilityRegistry {
    /// Creates an empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts or replaces one model capability declaration.
    pub fn insert(&mut self, model: String, capabilities: ModelCapabilities) {
        self.models.insert(model, capabilities);
    }

    /// Returns one model capability declaration.
    #[must_use]
    pub fn get(&self, model: &str) -> Option<&ModelCapabilities> {
        self.models.get(model)
    }

    /// Returns whether a model supports a capability.
    #[must_use]
    pub fn supports(&self, model: &str, capability: Capability) -> bool {
        self.get(model)
            .is_some_and(|capabilities| capabilities.supports(capability))
    }

    /// Returns the number of known models.
    #[must_use]
    pub fn len(&self) -> usize {
        self.models.len()
    }

    /// Returns whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.models.is_empty()
    }
}

impl ModelDriverUseCase {
    /// Parses one route-table key.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "default" => Some(Self::Default),
            "exec" => Some(Self::Exec),
            "socket" => Some(Self::Socket),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }

    /// Returns the route-table key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Exec => "exec",
            Self::Socket => "socket",
            Self::Agent => "agent",
        }
    }
}

impl ModelDriverRoutingTable {
    /// Creates an empty driver routing table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts one ordered route list.
    pub fn insert(&mut self, use_case: ModelDriverUseCase, drivers: Vec<String>) {
        self.routes.insert(use_case, drivers);
    }

    /// Returns the exact route list for one use case.
    #[must_use]
    pub fn get(&self, use_case: ModelDriverUseCase) -> Option<&[String]> {
        self.routes.get(&use_case).map(Vec::as_slice)
    }

    /// Returns the route list for a use case, falling back to `default`.
    #[must_use]
    pub fn drivers_for(&self, use_case: ModelDriverUseCase) -> Option<&[String]> {
        self.get(use_case)
            .or_else(|| self.get(ModelDriverUseCase::Default))
    }

    /// Returns the first selected driver for a use case.
    #[must_use]
    pub fn primary_driver_for(&self, use_case: ModelDriverUseCase) -> Option<&str> {
        self.drivers_for(use_case)
            .and_then(|drivers| drivers.first())
            .map(String::as_str)
    }

    /// Returns whether no route is present.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    fn route_value(&self, use_case: ModelDriverUseCase) -> String {
        self.get(use_case)
            .map(|drivers| drivers.join(","))
            .unwrap_or_default()
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
        if let Some(attr) = self.virtual_model_attr(&normalized)? {
            return Ok(attr);
        }
        if let Some(attr) = self.virtual_tool_attr(&normalized)? {
            return Ok(attr);
        }
        let path = self.resolve(&normalized)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| fuse_metadata_error(&error))?;
        if let Some(content) = self.virtual_exec_content(&normalized)? {
            return Ok(FuseV1Attr::with_owner(
                normalized,
                fuse_file_type(metadata.file_type()),
                u64::try_from(content.len()).map_err(|_error| FuseV1Error::Io)?,
                (metadata.permissions().mode() & !0o222) | 0o555,
                metadata.uid(),
                metadata.gid(),
            ));
        }
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
        if let Some(content) = self.virtual_model_content(&normalized)? {
            return Ok(content);
        }
        if let Some(content) = self.virtual_tool_content(&normalized)? {
            return Ok(content);
        }
        if let Some(content) = self.virtual_exec_content(&normalized)? {
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
        if let Some(content) = self.virtual_model_content(&normalized)? {
            return read_bytes_at(content.as_bytes(), offset, size);
        }
        if let Some(content) = self.virtual_tool_content(&normalized)? {
            return read_bytes_at(content.as_bytes(), offset, size);
        }
        if let Some(content) = self.virtual_exec_content(&normalized)? {
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

    fn virtual_exec_content(&self, abi_path: &str) -> Result<Option<String>, FuseV1Error> {
        let Some(model_name) = model_exec_name(abi_path) else {
            return Ok(None);
        };
        let exec_path = self.root.join("model").join(model_name);
        if !exec_path.exists() {
            return Ok(None);
        }
        Ok(Some(model_exec_metadata(
            model_name,
            &self.root.join("model").join(format!("{model_name}.d")),
        )?))
    }

    fn virtual_model_attr(&self, abi_path: &str) -> Result<Option<FuseV1Attr>, FuseV1Error> {
        let Some((file_type, size, mode)) = self.virtual_model_entry(abi_path)? else {
            return Ok(None);
        };
        Ok(Some(FuseV1Attr::with_owner(
            abi_path.to_owned(),
            file_type,
            size,
            mode,
            0,
            0,
        )))
    }

    fn virtual_tool_attr(&self, abi_path: &str) -> Result<Option<FuseV1Attr>, FuseV1Error> {
        let Some(tool_name) = tool_exec_name(abi_path) else {
            return Ok(None);
        };
        let control_dir = self.root.join("tool").join(format!("{tool_name}.d"));
        if !control_dir.is_dir() {
            return Ok(None);
        }
        let content = tool_exec_metadata(tool_name, &control_dir)?;
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

    fn virtual_tool_content(&self, abi_path: &str) -> Result<Option<String>, FuseV1Error> {
        let Some(tool_name) = tool_exec_name(abi_path) else {
            return Ok(None);
        };
        let control_dir = self.root.join("tool").join(format!("{tool_name}.d"));
        if !control_dir.is_dir() {
            return Ok(None);
        }
        Ok(Some(tool_exec_metadata(tool_name, &control_dir)?))
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

/// Parses one v1 JSONL socket request frame.
///
/// Unknown fields are ignored by design. Only the stable fields that affect
/// `CortexFS` session semantics are consumed.
pub fn parse_socket_request_frame(frame: &str) -> Result<SocketRequest, SocketRequestError> {
    if frame.len() > MAX_SOCKET_FRAME_BYTES {
        return Err(SocketRequestError::FrameTooLarge { bytes: frame.len() });
    }

    let frame = trim_jsonl_frame(frame)?;
    if !frame.trim_start().starts_with('{') {
        return Err(SocketRequestError::RequestNotObject);
    }
    let request = serde_path_to_error::deserialize::<_, SocketRequestFrame>(
        &mut serde_json::Deserializer::from_str(frame),
    )
    .map_err(|error| socket_request_deserialize_error(&error, frame))?;
    request.try_into()
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

#[derive(Deserialize)]
#[serde(tag = "op")]
enum SocketRequestFrame {
    #[serde(rename = "send")]
    Send {
        id: String,
        #[serde(default = "default_socket_session")]
        session: String,
        #[serde(default)]
        scope: SocketSessionScopeFrame,
        cwd: Option<String>,
        input: String,
    },
    #[serde(rename = "resume")]
    Resume {
        #[serde(default = "default_socket_session")]
        session: String,
        after: Option<String>,
    },
    #[serde(rename = "cancel")]
    Cancel { id: String },
    #[serde(rename = "ping")]
    Ping,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SocketSessionScopeFrame {
    #[default]
    Private,
    Shared,
    Temp,
}

impl From<SocketSessionScopeFrame> for SocketSessionScope {
    fn from(scope: SocketSessionScopeFrame) -> Self {
        match scope {
            SocketSessionScopeFrame::Private => Self::Private,
            SocketSessionScopeFrame::Shared => Self::Shared,
            SocketSessionScopeFrame::Temp => Self::Temp,
        }
    }
}

impl TryFrom<SocketRequestFrame> for SocketRequest {
    type Error = SocketRequestError;

    fn try_from(request: SocketRequestFrame) -> Result<Self, Self::Error> {
        match request {
            SocketRequestFrame::Send {
                id,
                session,
                scope,
                cwd,
                input,
            } => parse_socket_send_request(id, session, scope.into(), cwd, input),
            SocketRequestFrame::Resume { session, after } => {
                parse_socket_resume_request(session, after)
            }
            SocketRequestFrame::Cancel { id } => parse_socket_cancel_request(id),
            SocketRequestFrame::Ping => Ok(Self::Ping),
        }
    }
}

fn parse_socket_send_request(
    id: String,
    session: String,
    scope: SocketSessionScope,
    cwd: Option<String>,
    input: String,
) -> Result<SocketRequest, SocketRequestError> {
    validate_socket_object_field("id", &id)?;
    validate_socket_object_field("session", &session)?;
    validate_optional_socket_cwd(cwd.as_deref())?;
    if input.contains('\0') {
        return Err(SocketRequestError::InvalidField {
            field: "input",
            value: input,
        });
    }

    Ok(SocketRequest::Send {
        id,
        session,
        scope,
        cwd,
        input,
    })
}

fn parse_socket_resume_request(
    session: String,
    after: Option<String>,
) -> Result<SocketRequest, SocketRequestError> {
    validate_socket_object_field("session", &session)?;
    validate_optional_socket_object_field("after", after.as_deref())?;
    Ok(SocketRequest::Resume { session, after })
}

fn parse_socket_cancel_request(id: String) -> Result<SocketRequest, SocketRequestError> {
    validate_socket_object_field("id", &id)?;
    Ok(SocketRequest::Cancel { id })
}

fn default_socket_session() -> String {
    "default".to_owned()
}

fn validate_optional_socket_cwd(cwd: Option<&str>) -> Result<(), SocketRequestError> {
    if let Some(cwd) = cwd
        && !is_stable_chroot_absolute_path(cwd)
    {
        return Err(SocketRequestError::InvalidField {
            field: "cwd",
            value: cwd.to_owned(),
        });
    }
    Ok(())
}

fn validate_optional_socket_object_field(
    field: &'static str,
    value: Option<&str>,
) -> Result<(), SocketRequestError> {
    if let Some(value) = value {
        validate_socket_object_field(field, value)?;
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

fn socket_request_deserialize_error(
    error: &serde_path_to_error::Error<serde_json::Error>,
    frame: &str,
) -> SocketRequestError {
    if error.inner().is_syntax() || error.inner().is_eof() {
        return SocketRequestError::InvalidJson;
    }
    if let Some(error) = socket_request_stable_field_error(frame) {
        return error;
    }
    let message = error.inner().to_string();
    if message.contains("missing field `op`") {
        return SocketRequestError::MissingOp;
    }
    if message.contains("unknown variant")
        && message.contains("private")
        && message.contains("shared")
        && message.contains("temp")
    {
        return socket_request_scope_error(message);
    }
    match error.path().to_string().as_str() {
        "." => SocketRequestError::RequestNotObject,
        "op" => socket_request_unknown_op_error(message.as_str())
            .unwrap_or(SocketRequestError::MissingOp),
        "id" => SocketRequestError::MissingStringField("id"),
        "session" => SocketRequestError::MissingStringField("session"),
        "scope" => socket_request_scope_error(message),
        "cwd" => SocketRequestError::MissingStringField("cwd"),
        "after" => SocketRequestError::MissingStringField("after"),
        "input" => SocketRequestError::MissingStringField("input"),
        _ => SocketRequestError::InvalidJson,
    }
}

fn socket_request_stable_field_error(frame: &str) -> Option<SocketRequestError> {
    let value = serde_json::from_str::<Value>(frame).ok()?;
    let object = value.as_object()?;
    let op = object.get("op")?.as_str()?;
    match op {
        "send" => socket_string_field_error(object, "id")
            .or_else(|| socket_string_field_error(object, "session"))
            .or_else(|| socket_string_field_error(object, "scope"))
            .or_else(|| socket_scope_value_error(object))
            .or_else(|| socket_string_field_error(object, "cwd"))
            .or_else(|| socket_string_field_error(object, "input")),
        "resume" => socket_string_field_error(object, "session")
            .or_else(|| socket_string_field_error(object, "after")),
        "cancel" => socket_string_field_error(object, "id"),
        _ => None,
    }
}

fn socket_string_field_error(
    object: &serde_json::Map<String, Value>,
    field: &'static str,
) -> Option<SocketRequestError> {
    object
        .get(field)
        .filter(|value| !value.is_string())
        .map(|_value| SocketRequestError::MissingStringField(field))
}

fn socket_scope_value_error(object: &serde_json::Map<String, Value>) -> Option<SocketRequestError> {
    let scope = object.get("scope")?.as_str()?;
    SocketSessionScope::parse(scope)
        .is_none()
        .then(|| SocketRequestError::InvalidField {
            field: "scope",
            value: scope.to_owned(),
        })
}

fn socket_request_scope_error(message: String) -> SocketRequestError {
    let value = quoted_json_error_value(&message).unwrap_or(message);
    SocketRequestError::InvalidField {
        field: "scope",
        value,
    }
}

fn socket_request_unknown_op_error(message: &str) -> Option<SocketRequestError> {
    if !message.contains("unknown variant") {
        return None;
    }
    let value = quoted_json_error_value(message)?;
    Some(SocketRequestError::UnknownOp(value))
}

fn quoted_json_error_value(message: &str) -> Option<String> {
    let start = message.find('`')? + 1;
    let end = message.get(start..)?.find('`')? + start;
    message.get(start..end).map(ToOwned::to_owned)
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

    let agent_frames = run_agent_executable_streaming(
        stream,
        runtime.agent_executable,
        runtime.agent_name,
        id,
        session,
        input,
    )?;
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
    agent_executable: &Path,
    agent_name: &str,
    run_id: &str,
    session: &str,
    input: &str,
) -> Result<Vec<String>, SocketRuntimeError> {
    let mut child = Command::new(agent_executable)
        .arg(input)
        .env("CTX_AGENT", agent_name)
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
    output.push_str("session: ");
    output.push_str(session);
    output.push('\n');
    if let Some(agent) = agent {
        output.push_str("agent: ");
        output.push_str(agent);
        output.push('\n');
    }
    if let Some(budget) = budget {
        output.push_str("budget_tokens: ");
        output.push_str(&budget.to_string());
        output.push('\n');
    }
    output.push('\n');

    for candidate in candidates {
        output.push_str("## ");
        output.push_str(&candidate.kind);
        output.push_str("\n\nsource: ");
        output.push_str(&candidate.source);
        output.push('\n');
        if let Some(range) = candidate.range.as_deref() {
            output.push_str("range: ");
            output.push_str(range);
            output.push('\n');
        }
        output.push_str("tokens: ");
        output.push_str(&candidate.tokens.to_string());
        output.push_str("\n\n");
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

/// Inspects a `model/<provider>/<model>.d/cap` file body for stable v1 capability words.
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

/// Parses `model/<provider>/<model>.d/driver`.
///
/// A legacy single-line value such as `debug` is treated as `default=debug`.
/// Route-table form supports `default`, `exec`, `socket`, and `agent` keys with
/// comma-separated drivers in priority order.
pub fn parse_model_driver_routes(
    content: &str,
) -> Result<ModelDriverRoutingTable, ModelDriverRouteError> {
    let significant = content
        .lines()
        .enumerate()
        .filter_map(|(index, line)| {
            let value = line.trim();
            (!value.is_empty() && !value.starts_with('#')).then_some((index + 1, value))
        })
        .collect::<Vec<_>>();

    if significant.is_empty() {
        return Err(ModelDriverRouteError::Empty);
    }

    if significant.len() == 1 {
        let Some((line, driver)) = significant.first().copied() else {
            return Err(ModelDriverRouteError::Empty);
        };
        if !driver.contains('=') {
            return parse_driver_list(line, driver).map(|drivers| {
                let mut table = ModelDriverRoutingTable::new();
                table.insert(ModelDriverUseCase::Default, drivers);
                table
            });
        }
    }

    let mut table = ModelDriverRoutingTable::new();
    for (line, route) in significant {
        let Some((raw_key, raw_drivers)) = route.split_once('=') else {
            return Err(ModelDriverRouteError::MissingEquals { line });
        };
        let key = raw_key.trim();
        let Some(use_case) = ModelDriverUseCase::parse(key) else {
            return Err(ModelDriverRouteError::UnknownUseCase {
                line,
                value: key.to_owned(),
            });
        };
        if table.get(use_case).is_some() {
            return Err(ModelDriverRouteError::DuplicateUseCase {
                line,
                value: key.to_owned(),
            });
        }
        table.insert(use_case, parse_driver_list(line, raw_drivers)?);
    }

    if table.is_empty() {
        Err(ModelDriverRouteError::Empty)
    } else {
        Ok(table)
    }
}

fn parse_driver_list(line: usize, value: &str) -> Result<Vec<String>, ModelDriverRouteError> {
    let mut drivers = Vec::new();
    for raw_driver in value.split(',') {
        let driver = raw_driver.trim();
        if driver.is_empty() {
            return Err(ModelDriverRouteError::EmptyDriver { line });
        }
        if !is_object_name(driver) {
            return Err(ModelDriverRouteError::InvalidDriverName {
                line,
                value: driver.to_owned(),
            });
        }
        drivers.push(driver.to_owned());
    }
    if drivers.is_empty() {
        Err(ModelDriverRouteError::EmptyDriver { line })
    } else {
        Ok(drivers)
    }
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

    let mut issues = Vec::new();
    if !jsonschema::meta::is_valid(&value) {
        issues.push(ToolSchemaIssue::InvalidSchema);
    }
    issues.extend(
        object
            .keys()
            .filter(|field| is_tool_schema_authority_field(field))
            .map(|field| ToolSchemaIssue::AuthorityField(field.clone())),
    );
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
    Ok(format!(
        "#!{CORTEXFS_OBJECT_RUNNER}\n\
         # cortexfs.object=model\n\
         # cortexfs.id={id}\n\
         # cortexfs.name={name}\n\
         # cortexfs.description={description}\n\
         # cortexfs.type={model_type}\n\
         # cortexfs.created_at=\n\
         # cortexfs.owned_by={owned_by}\n\
         # cortexfs.context_length={context_length}\n\
         # cortexfs.driver={driver}\n\
         # cortexfs.driver.default={}\n\
         # cortexfs.driver.exec={}\n\
         # cortexfs.driver.socket={}\n\
         # cortexfs.driver.agent={}\n\
         # cortexfs.session={session}\n\
         # cortexfs.status={status}\n\
         # cortexfs.cap={}\n",
        driver_routes.route_value(ModelDriverUseCase::Default),
        driver_routes.route_value(ModelDriverUseCase::Exec),
        driver_routes.route_value(ModelDriverUseCase::Socket),
        driver_routes.route_value(ModelDriverUseCase::Agent),
        cap.lines().collect::<Vec<_>>().join(",")
    ))
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
    Ok(format!(
        "#!{CORTEXFS_OBJECT_RUNNER}\n\
         # cortexfs.object=tool\n\
         # cortexfs.name={name}\n\
         # cortexfs.declared_name={declared_name}\n\
         # cortexfs.description={description}\n\
         # cortexfs.runner=cortexfs-object-runner\n\
         # cortexfs.status={status}\n\
         # cortexfs.cap={}\n",
        cap.lines().collect::<Vec<_>>().join(",")
    ))
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
root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
run="${{CTX_RUN_ID:-r1}}"
input="$*"
if [ -z "$input" ]; then
  input="$(cat)"
fi
model="$(tr -d '\n' < "$root/agent/{name}.d/model" 2>/dev/null || true)"
if [ -z "$model" ]; then
  model="debug/echo"
fi
if [ ! -x "$root/model/$model" ]; then
  printf '{{"type":"error","run":"%s","code":"ENOENT","message":"missing model"}}\n' "$run"
  printf '{{"type":"done","run":"%s","status":"error"}}\n' "$run"
  exit 1
fi
CTX_RUN_ID="$run" exec "$root/model/$model" "$input"
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

fn tool_exec_name(abi_path: &str) -> Option<&str> {
    let name = abi_path.strip_prefix("tool/")?;
    if name.contains('/') {
        return None;
    }
    if name
        .rsplit_once('.')
        .is_some_and(|(_stem, suffix)| matches!(suffix, "d" | "sock"))
    {
        return None;
    }
    is_object_name(name).then_some(name)
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
    let Ok(pack) = serde_path_to_error::deserialize::<_, ContextPackJson>(
        &mut serde_json::Deserializer::from_str(content),
    ) else {
        if serde_json::from_str::<Value>(content).is_ok() {
            return ContextPackReport::new(vec![ContextPackIssue::ItemsNotArray]);
        }
        return ContextPackReport::new(vec![ContextPackIssue::InvalidJson]);
    };

    let mut issues = Vec::new();
    for (index, item) in pack.items.iter().enumerate() {
        inspect_context_pack_item(index, item, &mut issues);
    }

    ContextPackReport::new(issues)
}

#[derive(Deserialize)]
struct ContextPackJson {
    items: Vec<ContextPackItemJson>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ContextPackItemJson {
    Object {
        source: Option<ContextPackSourceJson>,
    },
    Other(Value),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ContextPackSourceJson {
    String(String),
    Other(Value),
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

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonU64Field {
    Number(u64),
    Other(Value),
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum JsonStringArrayField {
    Strings(Vec<String>),
    Other(Value),
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
    let Ok(message) = serde_path_to_error::deserialize::<_, MessageLineJson>(
        &mut serde_json::Deserializer::from_str(line),
    ) else {
        issues.push(MessageStreamIssue::MessageNotObject(line_number));
        return;
    };

    append_provider_native_message_field_issues(line_number, &value, issues);

    let Some(role) = message.role.as_ref().and_then(JsonStringField::as_str) else {
        issues.push(MessageStreamIssue::MissingRole(line_number));
        return;
    };
    if !matches!(role, "system" | "user" | "assistant" | "tool") {
        issues.push(MessageStreamIssue::InvalidRole {
            line: line_number,
            role: role.to_owned(),
        });
    }

    let Some(content) = message.content.as_ref() else {
        issues.push(MessageStreamIssue::MissingContent(line_number));
        return;
    };
    if !serde_json::from_value::<MessageContentJson>(content.clone())
        .is_ok_and(|content| content.is_well_formed())
    {
        issues.push(MessageStreamIssue::InvalidContent(line_number));
    }
}

#[derive(Deserialize)]
struct MessageLineJson {
    role: Option<JsonStringField>,
    content: Option<Value>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum MessageContentJson {
    Text(String),
    Parts(Vec<MessageContentPartJson>),
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum MessageContentPartJson {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { path: String },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_call_id: String,
        content: MessageContentJson,
    },
}

impl MessageContentJson {
    fn is_well_formed(&self) -> bool {
        match *self {
            Self::Text(ref text) => {
                let _ = text;
                true
            }
            Self::Parts(ref parts) => parts.iter().all(MessageContentPartJson::is_well_formed),
        }
    }
}

impl MessageContentPartJson {
    fn is_well_formed(&self) -> bool {
        match *self {
            Self::Text { ref text } => {
                let _ = text;
                true
            }
            Self::Image { ref path } => {
                let _ = path;
                true
            }
            Self::ToolResult {
                ref tool_call_id,
                ref content,
            } => {
                let _ = tool_call_id;
                content.is_well_formed()
            }
        }
    }
}

fn append_provider_native_message_field_issues(
    line_number: usize,
    value: &Value,
    issues: &mut Vec<MessageStreamIssue>,
) {
    for field in provider_native_fields(value) {
        issues.push(MessageStreamIssue::ProviderNativeField {
            line: line_number,
            field: field.to_owned(),
        });
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
    if !line.trim_start().starts_with('{') {
        if serde_json::from_str::<Value>(line).is_ok() {
            issues.push(ContextJsonlIssue::RecordNotObject(line_number));
        } else {
            issues.push(ContextJsonlIssue::InvalidJson(line_number));
        }
        return;
    }
    let Ok(record) = serde_path_to_error::deserialize::<_, ContextJsonlRecordJson>(
        &mut serde_json::Deserializer::from_str(line),
    ) else {
        issues.push(ContextJsonlIssue::InvalidJson(line_number));
        return;
    };

    match kind {
        ContextJsonlKind::Facts => inspect_fact_record(line_number, &record, issues),
        ContextJsonlKind::Decisions => inspect_decision_record(line_number, &record, issues),
        ContextJsonlKind::Refs => inspect_ref_record(line_number, &record, issues),
        ContextJsonlKind::SwapIndex => inspect_swap_index_record(line_number, &record, issues),
        ContextJsonlKind::DedupIndex => inspect_dedup_index_record(line_number, &record, issues),
    }
}

#[derive(Deserialize)]
struct ContextJsonlRecordJson {
    id: Option<JsonStringField>,
    text: Option<JsonStringField>,
    decision: Option<JsonStringField>,
    source: Option<JsonStringField>,
    path: Option<JsonStringField>,
    kind: Option<JsonStringField>,
    summary: Option<JsonStringField>,
    tokens: Option<JsonU64Field>,
    hash: Option<JsonStringField>,
    refs: Option<JsonStringArrayField>,
    bytes: Option<JsonU64Field>,
}

fn inspect_fact_record(
    line: usize,
    record: &ContextJsonlRecordJson,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(line, record.id.as_ref(), "id", issues, is_context_record_id);
    require_context_string_field(
        line,
        record.text.as_ref(),
        "text",
        issues,
        is_nonempty_single_line,
    );
    require_context_string_field(
        line,
        record.source.as_ref(),
        "source",
        issues,
        is_nonempty_single_line,
    );
}

fn inspect_decision_record(
    line: usize,
    record: &ContextJsonlRecordJson,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(line, record.id.as_ref(), "id", issues, is_context_record_id);
    require_context_string_field(
        line,
        record.decision.as_ref(),
        "decision",
        issues,
        is_nonempty_single_line,
    );
    require_context_string_field(
        line,
        record.source.as_ref(),
        "source",
        issues,
        is_nonempty_single_line,
    );
}

fn inspect_ref_record(
    line: usize,
    record: &ContextJsonlRecordJson,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(line, record.id.as_ref(), "id", issues, is_context_record_id);
    require_context_string_field(
        line,
        record.path.as_ref(),
        "path",
        issues,
        is_stable_context_ref_path,
    );
    require_context_string_field(
        line,
        record.kind.as_ref(),
        "kind",
        issues,
        is_context_ref_kind,
    );
    require_context_string_field(
        line,
        record.summary.as_ref(),
        "summary",
        issues,
        is_nonempty_single_line,
    );
}

fn inspect_swap_index_record(
    line: usize,
    record: &ContextJsonlRecordJson,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(line, record.id.as_ref(), "id", issues, is_context_hash_id);
    require_context_string_field(line, record.kind.as_ref(), "kind", issues, is_swap_kind);
    require_context_string_field(
        line,
        record.source.as_ref(),
        "source",
        issues,
        is_swap_source,
    );
    require_context_string_field(
        line,
        record.summary.as_ref(),
        "summary",
        issues,
        is_nonempty_single_line,
    );
    require_context_number_field(line, record.tokens.as_ref(), "tokens", issues);
}

fn inspect_dedup_index_record(
    line: usize,
    record: &ContextJsonlRecordJson,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    require_context_string_field(
        line,
        record.hash.as_ref(),
        "hash",
        issues,
        is_context_hash_id,
    );
    require_context_string_array_field(
        line,
        record.refs.as_ref(),
        "refs",
        issues,
        is_nonempty_single_line,
    );
    require_context_number_field(line, record.bytes.as_ref(), "bytes", issues);
    require_context_number_field(line, record.tokens.as_ref(), "tokens", issues);
}

fn require_context_string_field(
    line: usize,
    value: Option<&JsonStringField>,
    field: &str,
    issues: &mut Vec<ContextJsonlIssue>,
    valid: impl Fn(&str) -> bool,
) {
    let Some(value) = value.and_then(JsonStringField::as_str) else {
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
    values: Option<&JsonStringArrayField>,
    field: &str,
    issues: &mut Vec<ContextJsonlIssue>,
    valid: impl Fn(&str) -> bool,
) {
    let Some(values) = json_string_array_values(values) else {
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
        if !valid(value) {
            issues.push(ContextJsonlIssue::InvalidField {
                line,
                field: field.to_owned(),
                value: value.clone(),
            });
        }
    }
}

fn require_context_number_field(
    line: usize,
    value: Option<&JsonU64Field>,
    field: &str,
    issues: &mut Vec<ContextJsonlIssue>,
) {
    if !is_json_u64(value) {
        issues.push(ContextJsonlIssue::MissingNumberField {
            line,
            field: field.to_owned(),
        });
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

fn json_string_array_values(value: Option<&JsonStringArrayField>) -> Option<&[String]> {
    value.and_then(|value| match *value {
        JsonStringArrayField::Strings(ref values) => Some(values.as_slice()),
        JsonStringArrayField::Other(ref value) => {
            let _ = value;
            None
        }
    })
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
    let Ok(event) = serde_path_to_error::deserialize::<_, EventLineJson>(
        &mut serde_json::Deserializer::from_str(line),
    ) else {
        issues.push(EventStreamIssue::EventNotObject(line_number));
        return;
    };

    append_provider_native_field_issues(line_number, &value, issues);

    let Some(event_type) = event.event_type.as_ref().and_then(JsonStringField::as_str) else {
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
    if event_requires_run(event_type)
        && event
            .run
            .as_ref()
            .and_then(JsonStringField::as_str)
            .is_none()
    {
        issues.push(EventStreamIssue::MissingRun(line_number));
    }

    match event_type {
        "error" => inspect_error_event(line_number, &event, issues),
        "done" => inspect_done_event(line_number, &event, issues),
        "usage" => inspect_usage_event(line_number, &event, issues),
        "tool_call" => inspect_tool_call_event(line_number, &event, issues),
        "agent.child.cancel" => inspect_agent_child_cancel_event(line_number, &event, issues),
        "agent.stop" => inspect_agent_stop_event(line_number, &event, issues),
        _ => {}
    }
}

#[derive(Deserialize)]
struct EventLineJson {
    #[serde(rename = "type")]
    event_type: Option<JsonStringField>,
    run: Option<JsonStringField>,
    code: Option<JsonStringField>,
    status: Option<JsonStringField>,
    input_tokens: Option<JsonU64Field>,
    output_tokens: Option<JsonU64Field>,
    id: Option<JsonStringField>,
    name: Option<JsonStringField>,
    parent: Option<JsonStringField>,
    child: Option<JsonStringField>,
    reason: Option<JsonStringField>,
    agent: Option<JsonStringField>,
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
    for field in provider_native_fields(value) {
        issues.push(EventStreamIssue::ProviderNativeField {
            line: line_number,
            field: field.to_owned(),
        });
    }
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

fn inspect_error_event(
    line_number: usize,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    let Some(code) = event.code.as_ref().and_then(JsonStringField::as_str) else {
        issues.push(EventStreamIssue::InvalidErrorCode(line_number));
        return;
    };
    if !is_stable_errno(code) {
        issues.push(EventStreamIssue::InvalidErrorCode(line_number));
    }
}

fn inspect_done_event(
    line_number: usize,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    if !matches!(
        event.status.as_ref().and_then(JsonStringField::as_str),
        Some("ok" | "error" | "cancelled")
    ) {
        issues.push(EventStreamIssue::InvalidDoneStatus(line_number));
    }
}

fn inspect_usage_event(
    line_number: usize,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    if !is_json_u64(event.input_tokens.as_ref()) || !is_json_u64(event.output_tokens.as_ref()) {
        issues.push(EventStreamIssue::InvalidUsage(line_number));
    }
}

fn inspect_tool_call_event(
    line_number: usize,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    let valid_id = event
        .id
        .as_ref()
        .and_then(JsonStringField::as_str)
        .is_some_and(is_object_name);
    let valid_name = event
        .name
        .as_ref()
        .and_then(JsonStringField::as_str)
        .is_some_and(is_object_name);
    if !valid_id || !valid_name {
        issues.push(EventStreamIssue::InvalidToolCall(line_number));
    }
}

fn inspect_agent_child_cancel_event(
    line_number: usize,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    let parent = event.parent.as_ref().and_then(JsonStringField::as_str);
    let child = event.child.as_ref().and_then(JsonStringField::as_str);
    let reason = event.reason.as_ref().and_then(JsonStringField::as_str);
    if !parent.is_some_and(is_object_name)
        || !child.is_some_and(is_object_name)
        || reason != Some("parent_dead")
    {
        issues.push(EventStreamIssue::InvalidAgentLifecycle(line_number));
    }
}

fn inspect_agent_stop_event(
    line_number: usize,
    event: &EventLineJson,
    issues: &mut Vec<EventStreamIssue>,
) {
    let agent = event.agent.as_ref().and_then(JsonStringField::as_str);
    let status = event.status.as_ref().and_then(JsonStringField::as_str);
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

fn inspect_context_pack_item(
    index: usize,
    item: &ContextPackItemJson,
    issues: &mut Vec<ContextPackIssue>,
) {
    match *item {
        ContextPackItemJson::Other(ref value) => {
            let _ = value;
            issues.push(ContextPackIssue::ItemNotObject(index));
        }
        ContextPackItemJson::Object { source: None } => {
            issues.push(ContextPackIssue::MissingSource(index));
        }
        ContextPackItemJson::Object {
            source: Some(ContextPackSourceJson::Other(ref value)),
        } => {
            let _ = value;
            issues.push(ContextPackIssue::SourceNotString(index));
        }
        ContextPackItemJson::Object {
            source: Some(ContextPackSourceJson::String(ref source)),
        } => {
            if let Err(reason) = validate_context_pack_source(source) {
                issues.push(ContextPackIssue::InvalidSource {
                    item: index,
                    source: source.clone(),
                    reason,
                });
            }
        }
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
        let object_class = PolicyObjectClass::parse(class).ok_or(PolicyError::UnknownClass)?;
        let valid_object_name = match object_class {
            PolicyObjectClass::Model => is_model_name(object_name),
            PolicyObjectClass::Tool
            | PolicyObjectClass::Shared
            | PolicyObjectClass::Session
            | PolicyObjectClass::Mount
            | PolicyObjectClass::Agent
            | PolicyObjectClass::Network => is_object_name(object_name),
        };
        if !valid_object_name {
            return Err(PolicyError::InvalidName);
        }
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

/// Returns whether a model name uses the provider/model namespace shape.
#[must_use]
pub fn is_model_name(name: &str) -> bool {
    let Some((provider, model)) = name.split_once('/') else {
        return false;
    };
    !model.contains('/') && is_object_name(provider) && is_object_name(model)
}

fn is_object_name_for_class(class: ObjectClass, name: &str) -> bool {
    match class {
        ObjectClass::Model => is_model_name(name),
        ObjectClass::Agent | ObjectClass::Tool => is_object_name(name),
    }
}

/// Parsed `CortexFS` ABI path shape.
///
/// This is the typed companion to the stable `ctx.*` strings returned by
/// [`classify_abi_path`]. It is intended for internal routing and validation;
/// the filesystem ABI remains the path shape and stable type strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AbiPathKind<'a> {
    /// Path does not match any stable v1 ABI shape.
    Unknown,
    /// `model/<provider>`.
    ModelDir { provider: &'a str },
    /// `model/<provider>/<model>`, `agent/<name>`, or `tool/<name>`.
    ObjectExec {
        class: ObjectClass,
        provider: Option<&'a str>,
        name: &'a str,
    },
    /// `model/<provider>/<model>.sock`, `agent/<name>.sock`, or `tool/<name>.sock`.
    ObjectSocket {
        class: ObjectClass,
        provider: Option<&'a str>,
        name: &'a str,
    },
    /// Object control file path under `<name>.d/`.
    ObjectControl {
        class: ObjectClass,
        provider: Option<&'a str>,
        name: &'a str,
        file: &'a str,
    },
    /// `home/<uid>` and valid descendants not otherwise classified.
    HomeDir,
    /// Durable session root, for example `home/<uid>/agent/<agent>/session`.
    SessionRoot,
    /// Durable session instance directory.
    SessionDir { session: &'a str },
    /// Direct durable session file.
    SessionFile { session: &'a str, file: &'a str },
    /// Reserved durable session index file.
    SessionIndex { kind: SessionIndexKind },
    /// Durable file below `context/` in a session.
    SessionContextFile {
        session: &'a str,
        first: &'a str,
        second: Option<&'a str>,
    },
    /// `shared/<space>` and valid descendants not otherwise classified.
    SharedDir { space: &'a str },
    /// `shared/<space>/tool/<tool>`.
    SharedToolExec { space: &'a str, name: &'a str },
    /// `shared/<space>/tool/<tool>.d/<file>`.
    SharedToolControl {
        space: &'a str,
        name: &'a str,
        file: &'a str,
    },
    /// `shared/<space>/queue`.
    SharedQueueRoot { space: &'a str },
    /// A fixed child directory below `shared/<space>/queue`.
    SharedQueueDir { space: &'a str, name: &'a str },
    /// `shared/<space>/result`.
    SharedResult { space: &'a str },
    /// A syntactically valid ordinary file under an ABI-owned subtree.
    Ordinary,
}

impl AbiPathKind<'_> {
    /// Returns the stable `ctx file classify` string for this parsed path.
    #[must_use]
    pub fn stable_type(self) -> &'static str {
        match self {
            Self::Unknown => "ctx.unknown",
            Self::ModelDir { .. } => "ctx.model.dir",
            Self::ObjectExec { class, .. } => class.exec_type(),
            Self::ObjectSocket { class, .. } => class.socket_type(),
            Self::ObjectControl { class, .. } => class.control_type(),
            Self::HomeDir => "ctx.home.dir",
            Self::SessionRoot | Self::SessionDir { .. } => "ctx.session.dir",
            Self::SessionFile {
                file: "messages.jsonl",
                ..
            } => "ctx.session.messages",
            Self::SessionFile {
                file: "events.jsonl",
                ..
            } => "ctx.session.events",
            Self::SessionFile { .. }
            | Self::SessionIndex { .. }
            | Self::SessionContextFile { .. }
            | Self::Ordinary => "ctx.ordinary",
            Self::SharedDir { .. } => "ctx.shared.dir",
            Self::SharedToolExec { .. } => "ctx.shared.tool.exec",
            Self::SharedToolControl { .. } => "ctx.shared.tool.control",
            Self::SharedQueueRoot { .. } | Self::SharedQueueDir { .. } => "ctx.shared.queue",
            Self::SharedResult { .. } => "ctx.shared.result",
        }
    }
}

impl<'a> AbiPathKind<'a> {
    /// Returns an executable object class and stable name, when this path is an executable object.
    #[must_use]
    pub fn executable_object(self) -> Option<(ObjectClass, Cow<'a, str>)> {
        match self {
            Self::ObjectExec {
                class: ObjectClass::Model,
                provider: Some(provider),
                name,
            } => Some((ObjectClass::Model, Cow::Owned(format!("{provider}/{name}")))),
            Self::ObjectExec { class, name, .. } => Some((class, Cow::Borrowed(name))),
            _ => None,
        }
    }

    /// Returns a model control-file name for `model/<provider>/<model>.d/<file>`.
    #[must_use]
    pub const fn model_control_file(self) -> Option<&'a str> {
        match self {
            Self::ObjectControl {
                class: ObjectClass::Model,
                file,
                ..
            } => Some(file),
            _ => None,
        }
    }

    /// Returns a global or shared tool schema path.
    #[must_use]
    pub fn is_tool_schema(self) -> bool {
        matches!(
            self,
            Self::ObjectControl {
                class: ObjectClass::Tool,
                file: "schema",
                ..
            } | Self::SharedToolControl { file: "schema", .. }
        )
    }

    /// Returns a control file name for object `.d/` paths that carry policy or mount syntax.
    #[must_use]
    pub const fn control_file(self) -> Option<&'a str> {
        match self {
            Self::ObjectControl { file, .. } | Self::SharedToolControl { file, .. } => Some(file),
            _ => None,
        }
    }

    /// Returns the fixed agent-control kind, if this is `agent/<name>.d/<file>`.
    #[must_use]
    pub fn agent_control_kind(self) -> Option<AgentControlKind> {
        match self {
            Self::ObjectControl {
                class: ObjectClass::Agent,
                file,
                ..
            } => AgentControlKind::parse(file),
            _ => None,
        }
    }

    /// Returns the fixed session-index kind for reserved `session/index/*` files.
    #[must_use]
    pub fn session_index_kind(self) -> Option<SessionIndexKind> {
        match self {
            Self::SessionIndex { kind } => Some(kind),
            _ => None,
        }
    }

    /// Returns the fixed session control kind for direct session control files.
    #[must_use]
    pub fn session_control_kind(self) -> Option<SessionControlKind> {
        match self {
            Self::SessionFile { session, file } if is_object_name(session) => {
                SessionControlKind::parse(file)
            }
            _ => None,
        }
    }

    /// Returns whether this path is a durable session instance directory.
    #[must_use]
    pub fn is_session_instance(self) -> bool {
        matches!(self, Self::SessionDir { session } if is_object_name(session))
    }

    /// Returns a stable context JSONL kind for session `context/*` files.
    #[must_use]
    pub fn context_jsonl_kind(self) -> Option<ContextJsonlKind> {
        match self {
            Self::SessionContextFile {
                session,
                first,
                second,
            } if is_object_name(session) => match (first, second) {
                ("facts.jsonl", None) => Some(ContextJsonlKind::Facts),
                ("decisions.jsonl", None) => Some(ContextJsonlKind::Decisions),
                ("refs.jsonl", None) => Some(ContextJsonlKind::Refs),
                ("swap", Some("index.jsonl")) => Some(ContextJsonlKind::SwapIndex),
                ("dedup", Some("index.jsonl")) => Some(ContextJsonlKind::DedupIndex),
                _ => None,
            },
            _ => None,
        }
    }

    /// Returns whether this path is `context/pack.json` below a durable session.
    #[must_use]
    pub fn is_context_pack(self) -> bool {
        matches!(
            self,
            Self::SessionContextFile {
                session,
                first: "pack.json",
                second: None,
            } if is_object_name(session)
        )
    }
}

/// Classifies a relative `CortexFS` ABI path by path shape.
#[must_use]
pub fn classify_abi_path(path: &str) -> &'static str {
    parse_abi_path(path).stable_type()
}

/// Parses a relative `CortexFS` ABI path by path shape.
#[must_use]
pub fn parse_abi_path(path: &str) -> AbiPathKind<'_> {
    let trimmed = path.strip_prefix("./").map_or(path, |value| value);

    if trimmed.is_empty() || trimmed.split('/').any(str::is_empty) {
        return AbiPathKind::Unknown;
    }

    let parts = trimmed.split('/').collect::<Vec<_>>();
    let Some((first, rest)) = parts.split_first() else {
        return AbiPathKind::Unknown;
    };
    match *first {
        "model" => parse_model_object_path(rest),
        "agent" => parse_simple_object_path(ObjectClass::Agent, rest),
        "tool" => parse_simple_object_path(ObjectClass::Tool, rest),
        "home" => parse_home_path(rest),
        "shared" => parse_shared_path(rest),
        _ => AbiPathKind::Unknown,
    }
}

fn parse_simple_object_path<'a>(class: ObjectClass, parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((name, rest)) = parts.split_first() else {
        return AbiPathKind::Unknown;
    };

    if let Some(object_name) = name.strip_suffix(".sock") {
        return if rest.is_empty() && is_object_name(object_name) {
            AbiPathKind::ObjectSocket {
                class,
                provider: None,
                name: object_name,
            }
        } else {
            AbiPathKind::Unknown
        };
    }

    if let Some(object_name) = name.strip_suffix(".d") {
        return if let Some((file, _remaining)) = rest.split_first()
            && is_object_name(object_name)
        {
            AbiPathKind::ObjectControl {
                class,
                provider: None,
                name: object_name,
                file,
            }
        } else {
            AbiPathKind::Unknown
        };
    }

    if rest.is_empty() && is_object_name(name) {
        AbiPathKind::ObjectExec {
            class,
            provider: None,
            name,
        }
    } else {
        AbiPathKind::Unknown
    }
}

fn parse_model_object_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((provider, rest)) = parts.split_first() else {
        return AbiPathKind::Unknown;
    };
    if !is_object_name(provider) {
        return AbiPathKind::Unknown;
    }
    let Some((name, rest)) = rest.split_first() else {
        return AbiPathKind::ModelDir { provider };
    };

    if let Some(object_name) = name.strip_suffix(".sock") {
        return if rest.is_empty() && is_model_name(&format!("{provider}/{object_name}")) {
            AbiPathKind::ObjectSocket {
                class: ObjectClass::Model,
                provider: Some(provider),
                name: object_name,
            }
        } else {
            AbiPathKind::Unknown
        };
    }

    if let Some(object_name) = name.strip_suffix(".d") {
        return if let Some((file, _remaining)) = rest.split_first()
            && is_model_name(&format!("{provider}/{object_name}"))
        {
            AbiPathKind::ObjectControl {
                class: ObjectClass::Model,
                provider: Some(provider),
                name: object_name,
                file,
            }
        } else {
            AbiPathKind::Unknown
        };
    }

    let model_name = format!("{provider}/{name}");
    if rest.is_empty() && is_model_name(&model_name) {
        AbiPathKind::ObjectExec {
            class: ObjectClass::Model,
            provider: Some(provider),
            name,
        }
    } else {
        AbiPathKind::Unknown
    }
}

fn parse_home_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((_uid, rest)) = parts.split_first() else {
        return AbiPathKind::Unknown;
    };

    let Some((first, rest)) = rest.split_first() else {
        return AbiPathKind::HomeDir;
    };
    match *first {
        "agent" => parse_home_agent_path(rest),
        "model" => parse_home_model_path(rest),
        _ => AbiPathKind::HomeDir,
    }
}

fn parse_home_agent_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((agent, rest)) = parts.split_first() else {
        return AbiPathKind::HomeDir;
    };
    if !is_object_name(agent) {
        return AbiPathKind::Unknown;
    }
    let Some((first, rest)) = rest.split_first() else {
        return AbiPathKind::HomeDir;
    };
    match *first {
        "session" => parse_session_path(rest),
        _ => AbiPathKind::HomeDir,
    }
}

fn parse_home_model_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((provider, rest)) = parts.split_first() else {
        return AbiPathKind::HomeDir;
    };
    let Some((model_dir, rest)) = rest.split_first() else {
        return AbiPathKind::HomeDir;
    };
    let Some(model) = model_dir.strip_suffix(".d") else {
        return AbiPathKind::HomeDir;
    };
    if !is_model_name(&format!("{provider}/{model}")) {
        return AbiPathKind::Unknown;
    }
    let Some((first, rest)) = rest.split_first() else {
        return AbiPathKind::HomeDir;
    };
    match *first {
        "session" => parse_session_path(rest),
        _ => AbiPathKind::HomeDir,
    }
}

fn parse_shared_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((space, rest)) = parts.split_first() else {
        return AbiPathKind::Unknown;
    };
    if !is_object_name(space) {
        return AbiPathKind::Unknown;
    }
    let Some((first, rest)) = rest.split_first() else {
        return AbiPathKind::SharedDir { space };
    };
    match *first {
        "agent" => parse_shared_agent_path(space, rest),
        "model" => parse_shared_model_path(space, rest),
        "tool" => parse_shared_tool_path(space, rest),
        "queue" if rest.is_empty() => AbiPathKind::SharedQueueRoot { space },
        "queue" => parse_shared_queue_child(space, rest),
        "result" if rest.is_empty() => AbiPathKind::SharedResult { space },
        "result" => AbiPathKind::Ordinary,
        _ => AbiPathKind::SharedDir { space },
    }
}

fn parse_shared_queue_child<'a>(space: &'a str, rest: &[&'a str]) -> AbiPathKind<'a> {
    let Some((name, tail)) = rest.split_first() else {
        return AbiPathKind::SharedQueueRoot { space };
    };
    if tail.is_empty() && is_shared_queue_entry(name) {
        AbiPathKind::SharedQueueDir { space, name }
    } else {
        AbiPathKind::Ordinary
    }
}

fn parse_shared_tool_path<'a>(space: &'a str, parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((name, rest)) = parts.split_first() else {
        return AbiPathKind::SharedDir { space };
    };
    if let Some(tool_name) = name.strip_suffix(".d") {
        return if let Some((file, _remaining)) = rest.split_first()
            && is_object_name(tool_name)
        {
            AbiPathKind::SharedToolControl {
                space,
                name: tool_name,
                file,
            }
        } else {
            AbiPathKind::Unknown
        };
    }

    if rest.is_empty() && is_object_name(name) {
        AbiPathKind::SharedToolExec { space, name }
    } else {
        AbiPathKind::Unknown
    }
}

fn parse_shared_agent_path<'a>(space: &'a str, parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((agent, rest)) = parts.split_first() else {
        return AbiPathKind::SharedDir { space };
    };
    if !is_object_name(agent) {
        return AbiPathKind::Unknown;
    }
    let Some((first, rest)) = rest.split_first() else {
        return AbiPathKind::SharedDir { space };
    };
    match *first {
        "session" => parse_session_path(rest),
        _ => AbiPathKind::SharedDir { space },
    }
}

fn parse_shared_model_path<'a>(space: &'a str, parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((provider, rest)) = parts.split_first() else {
        return AbiPathKind::SharedDir { space };
    };
    let Some((model_dir, rest)) = rest.split_first() else {
        return AbiPathKind::SharedDir { space };
    };
    let Some(model) = model_dir.strip_suffix(".d") else {
        return AbiPathKind::SharedDir { space };
    };
    if !is_model_name(&format!("{provider}/{model}")) {
        return AbiPathKind::Unknown;
    }
    let Some((first, rest)) = rest.split_first() else {
        return AbiPathKind::SharedDir { space };
    };
    match *first {
        "session" => parse_session_path(rest),
        _ => AbiPathKind::SharedDir { space },
    }
}

fn parse_session_path<'a>(parts: &[&'a str]) -> AbiPathKind<'a> {
    let Some((session, rest)) = parts.split_first() else {
        return AbiPathKind::SessionRoot;
    };
    if !is_object_name(session) {
        return AbiPathKind::Unknown;
    }

    let Some((first, tail)) = rest.split_first() else {
        return AbiPathKind::SessionDir { session };
    };
    if *session == "index" {
        return if tail.is_empty() && *first == "list" {
            AbiPathKind::SessionIndex {
                kind: SessionIndexKind::List,
            }
        } else if tail.is_empty() && *first == "current" {
            AbiPathKind::SessionIndex {
                kind: SessionIndexKind::Current,
            }
        } else if *first == "by-cwd"
            && tail.len() == 1
            && tail.first().is_some_and(|hash| !hash.is_empty())
        {
            AbiPathKind::SessionIndex {
                kind: SessionIndexKind::ByCwd,
            }
        } else {
            AbiPathKind::Ordinary
        };
    }
    if *first == "context" {
        return parse_session_context_path(session, tail);
    }
    if tail.is_empty() {
        AbiPathKind::SessionFile {
            session,
            file: first,
        }
    } else {
        AbiPathKind::Ordinary
    }
}

fn parse_session_context_path<'a>(session: &'a str, tail: &[&'a str]) -> AbiPathKind<'a> {
    let Some((first, rest)) = tail.split_first() else {
        return AbiPathKind::Ordinary;
    };
    AbiPathKind::SessionContextFile {
        session,
        first,
        second: rest.first().copied(),
    }
}

const fn is_shared_queue_entry(name: &str) -> bool {
    matches!(
        name.as_bytes(),
        b"inbox" | b"pending" | b"lease" | b"claimed" | b"done" | b"failed"
    )
}

#[cfg(test)]
mod tests {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/unit/lib_tests.rs"
    ));
}
