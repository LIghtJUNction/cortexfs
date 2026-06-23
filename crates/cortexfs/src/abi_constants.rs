#![allow(
    clippy::redundant_pub_crate,
    reason = "private split module exposes internal constants to the parent crate"
)]

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

pub(super) const DEBUG_ECHO_MODEL: &str = "debug/echo";
pub(super) const DEBUG_ECHO_PROVIDER: &str = "debug";
pub(super) const DEBUG_ECHO_NAME: &str = "echo";
pub(super) const DEFAULT_MODEL_ALIAS: &str = "main";
pub(super) const HELPER_MODEL_ALIAS: &str = "helper";
pub(super) const DEFAULT_MODEL_ALIAS_TARGET: &str = "/ctx/model/debug/echo";
pub(super) const SYSTEM_PROVIDER_CONFIG_DIR: &str = "/etc/cortexfs/providers.d";
pub(super) const SYSTEM_PROVIDER_MODEL_CACHE_DIR: &str = "/var/lib/cortexfs/provider-models";

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

/// Maximum payload accepted by the v1 local FUSE projection for one small write.
pub const MAX_FUSE_V1_SMALL_WRITE_BYTES: usize = 64 * 1024;

/// Stable inode id for the v1 `/ctx` root in a FUSE adapter.
pub const FUSE_V1_ROOT_INODE: u64 = 1;
