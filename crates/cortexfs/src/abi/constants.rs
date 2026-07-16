#![allow(
    clippy::redundant_pub_crate,
    reason = "private split module exposes internal constants to the parent crate"
)]

/// Default `CortexFS` mount root.
pub const CTX_ROOT: &str = "/ctx";

/// Rust object runner used by executable object metadata files.
pub const CORTEXFS_OBJECT_RUNNER: &str = "/ctx/bin/cortexfs-object-runner";

/// Root entries reserved by the FUSE filesystem ABI.
pub const ROOT_ENTRIES: &[&str] = &["status", "bin", "model", "agent", "tool", "home", "shared"];

/// Object classes exposed as executable files.
pub const EXEC_OBJECTS: &[&str] = &["model", "agent", "tool"];

/// Maximum object name length.
pub const MAX_OBJECT_NAME_LEN: usize = 64;

/// Required model control files.
pub const MODEL_CONTROL_FILES: &[&str] = &[
    "id", "driver", "cap", "effort", "default", "fallback", "limit", "session", "status", "log",
];
/// Required hook directory inside every executable object's `.d/` control tree.
pub const OBJECT_HOOK_DIR: &str = "hooks";
/// Required hook phase directories inside `hooks/`.
pub const OBJECT_HOOK_PHASE_DIRS: &[&str] = &["pre.d", "post.d"];
pub(crate) const MODEL_ROUTE_FILE: &str = "route";
pub(crate) const DEFAULT_MODEL_ROUTE: &str = "\
# Global CortexFS model egress route.
# Rules are evaluated top to bottom. A group selects both transport and key slot.
# group(proxy) -> http(http://127.0.0.1:8080/v1), key(default)
# group(local-socket) -> unix(/run/user/1000/cortexfs/proxy/openai.sock), key(local)
# domain(bestproxy.com) -> proxy
# model(embedding-*) -> local-socket
# dip(geoip:private) -> direct
fallback: direct
";

pub(crate) const DEBUG_ECHO_MODEL: &str = "debug/echo";
pub(crate) const DEBUG_ECHO_PROVIDER: &str = "debug";
pub(crate) const DEBUG_ECHO_NAME: &str = "echo";
pub(crate) const DEFAULT_MODEL_ALIAS: &str = "main";
pub(crate) const HELPER_MODEL_ALIAS: &str = "helper";
/// Canonical model aliases exposed directly below `/ctx/model`.
pub const MODEL_ALIASES: &[&str] = &["main", "helper", "fast", "reason", "code", "vision"];
pub const DEFAULT_WORKER_MODEL: &str = "api.lmm.best/gpt-5.3-codex-spark";

/// Returns whether a name is a canonical model alias.
#[must_use]
pub fn is_model_alias(name: &str) -> bool {
    MODEL_ALIASES.contains(&name)
}

/// Returns the implicit model for an agent that has no explicit `model` control file.
#[must_use]
pub fn default_agent_model_for_name(agent_name: &str) -> &'static str {
    if is_worker_agent_name(agent_name) {
        DEFAULT_WORKER_MODEL
    } else {
        DEFAULT_MODEL_ALIAS
    }
}

/// Returns whether an agent name uses the v1 worker/executor role convention.
#[must_use]
pub fn is_worker_agent_name(agent_name: &str) -> bool {
    matches!(agent_name, "executor" | "worker") || is_dedicated_worker_agent_name(agent_name)
}

/// Returns whether an agent name is a dedicated worker/executor instance.
#[must_use]
pub fn is_dedicated_worker_agent_name(agent_name: &str) -> bool {
    agent_name.starts_with("executor-") || agent_name.starts_with("worker-")
}
pub(crate) const DEFAULT_MODEL_ALIAS_TARGET: &str = "/ctx/model/openai/gpt-5.6";
pub(crate) const HELPER_MODEL_ALIAS_TARGET: &str = "/ctx/model/openai/codex-auto-review";
pub(crate) const SYSTEM_PROVIDER_CONFIG_DIR: &str = "/etc/cortexfs/providers.d";
pub(crate) const SYSTEM_PROVIDER_MODEL_CACHE_DIR: &str = "/var/lib/cortexfs/provider-models";

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

/// Canonical agent control-file set materialized by bootstrap.
pub const AGENT_CONTROL_FILES: &[&str] = &[
    "abi",
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
    "window",
    "system.md",
    "prompt.template.md",
    "policy",
    "status",
    "pid",
    "log",
    "meta.json",
];

/// Optional agent control files recognized by v1.
///
/// Entries may overlap [`AGENT_CONTROL_FILES`] to preserve canonical bootstrap materialization.
pub const AGENT_OPTIONAL_CONTROL_FILES: &[&str] = &[
    "approval",
    "tools",
    "system.md",
    "prompt.template.md",
    "meta.json",
];

/// Default system prompt template for agent model calls.
pub const DEFAULT_AGENT_PROMPT_TEMPLATE: &str = r"# CortexFS Agent System Prompt

Time: {{current_time_unix}}
Agent: {{agent}}

## Runtime Contract

{{runtime_contract}}

## AGENT Instructions

{{agent_instructions}}

## Rules

{{rules}}

## Skills

{{skills}}

## Tool Injection

{{tool_injection}}

## History Messages

{{history_messages}}
";

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

/// Maximum payload returned by the v1 local FUSE projection for one small read.
pub const MAX_FUSE_V1_SMALL_READ_BYTES: u64 = 1024 * 1024;

/// Stable inode id for the v1 `/ctx` root in a FUSE adapter.
pub const FUSE_V1_ROOT_INODE: u64 = 1;
