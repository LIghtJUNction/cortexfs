use fuse3::Inode;
use std::time::Duration;

pub const ROOT_INODE: Inode = 1;

pub const STATUS_TEXT: &str = "ready\n";
pub const EMPTY_TEXT: &str = "";
pub const THREAD_COUNT_TEXT: &str = "1\n";
pub const DEFAULT_BATCH_FORMAT: &str = "openai.chat";
pub const TTL: Duration = Duration::from_secs(1);
pub const MAX_WRITE: u32 = 131_072;
pub const STATFS_BLOCK_SIZE: u32 = 512;
pub const STATFS_BLOCKS: u64 = 1024;
pub const STATFS_NAME_LENGTH: u32 = 255;
pub const DYNAMIC_INODE_BASE: Inode = 1_000_000;
pub const LOCAL_USER_ID: &str = "1000";
pub const LOCAL_USER_UID_TEXT: &str = "1000\n";
pub const LOCAL_USER_THREAD_DISPLAY_PATH: &str = "home/1000/thread/demo";
pub const LOCAL_USER_THREAD_DISPLAY_TEXT: &str = "home/1000/thread/demo\n";
pub const LOCAL_USER_MODELS_REFRESH_DISPLAY_TEXT: &str = "home/1000/model/refresh\n";
pub const LOCAL_USER_SPACE_CONTEXT_TEXT: &str = "local:uid1000:object_r:user_space_t:s0\n";
pub const LOCAL_USER_THREAD_CONTEXT_TEXT: &str = "local:uid1000:object_r:thread_t:s0\n";
pub const LOCAL_AGENT_CONTEXT_TEXT: &str = "local:uid1000:agent_r:agent_t:s0\n";
pub const LOCAL_USER_MEMORY_SCOPE_TEXT: &str = "home/1000:semantic\nhome/1000:profile\n";
pub const LOCAL_API_LISTEN_TEXT: &str = "127.0.0.1:6185\n";
pub const LOCAL_API_BASE_URL_TEXT: &str = "http://127.0.0.1:6185/v1\n";
pub const LOCAL_API_SOCKET_TEXT: &str = "/run/user/1000/cortex/api.sock\n";
pub const LOCAL_API_ENDPOINTS_TEXT: &str = "GET /v1/models\nPOST /v1/chat/completions\nPOST /v1/responses\nPOST /v1/messages\nPOST /v1/generateContent\n";
pub const LOCAL_API_PIPELINE_TEXT: &str = "normalize format\nroute\npolicy check\nsecret resolve\nprovider call\nstore response\nappend thread if bound\naudit\n";
pub const LOCAL_API_SOURCE_TEXT: &str = "home/1000/api\n";
pub const LOCAL_API_TRANSPORT_TEXT: &str = "fuse\nhttp\nunix\n";
pub const LOCAL_API_STORE_TEXT: &str = "cortex-store\n";
pub const LOCAL_API_POLICY_TEXT: &str = "cortex-policy\n";
pub const LOCAL_API_AUDIT_TEXT: &str = "audit/events.jsonl\n";
pub const AUDIT_DIR_PATH: &[&str] = &["audit"];
pub const API_PREFIX: &[&str] = &["home", "1000", "api"];
pub const BATCH_DIR_PATH: &[&str] = &["home", "1000", "batch"];
pub const BATCH_INBOX_PATH: &[&str] = &["home", "1000", "batch", "inbox"];
pub const BATCH_OUTBOX_PATH: &[&str] = &["home", "1000", "batch", "outbox"];
pub const CONTROL_DIR_PATH: &[&str] = &["control"];
pub const EXPORT_DIR_PATH: &[&str] = &["home", "1000", "export"];
pub const EXPORT_FILTERS_DIR_PATH: &[&str] = &["home", "1000", "export", "filter"];
pub const FEEDBACK_PREFERENCE_INBOX_PATH: &[&str] =
    &["home", "1000", "feedback", "preference", "inbox"];
pub const FEEDBACK_PREFERENCE_OUTBOX_PATH: &[&str] =
    &["home", "1000", "feedback", "preference", "outbox"];
pub const DEMO_THREAD_DIR_PATH: &[&str] = &["home", "1000", "thread", "demo"];
pub const DEMO_THREAD_CONTROL_PATH: &[&str] = &["home", "1000", "thread", "demo", "control"];
pub const DEMO_THREAD_INBOX_PATH: &[&str] = &["home", "1000", "thread", "demo", "inbox"];
pub const DEMO_THREAD_TOOL_LOOP_PATH: &[&str] = &["home", "1000", "thread", "demo", "tool-loop"];
pub const DEMO_THREAD_TOOL_LOOP_CONTROL_PATH: &[&str] =
    &["home", "1000", "thread", "demo", "tool-loop", "control"];
pub const DEMO_THREAD_TOOL_LOOP_LIMITS_PATH: &[&str] =
    &["home", "1000", "thread", "demo", "tool-loop", "limit"];
pub const EXTERNAL_QQ_GROUP_THREAD_DIR_PATH: &[&str] =
    &["ext", "qq", "group", "888888", "thread", "demo"];
pub const EXTERNAL_QQ_GROUP_THREAD_INBOX_PATH: &[&str] =
    &["ext", "qq", "group", "888888", "thread", "demo", "inbox"];
pub const EXTERNAL_QQ_SUBJECT_QUOTA_DIR_PATH: &[&str] =
    &["ext", "qq", "group", "888888", "subject", "123456", "quota"];
#[cfg(test)]
pub const EXTERNAL_QQ_SUBJECT_QUOTA_REQUESTS_PATH: &[&str] = &[
    "ext", "qq", "group", "888888", "subject", "123456", "quota", "requests",
];
pub const MEMORY_SEARCH_DIR_PATH: &[&str] = &["home", "1000", "memory", "search"];
pub const MEMORY_SEMANTIC_DIR_PATH: &[&str] = &["home", "1000", "memory", "semantic"];
pub const USER_POLICY_DIR_PATH: &[&str] = &["home", "1000", "policy"];
pub const USER_ROUTES_DIR_PATH: &[&str] = &["home", "1000", "route"];
pub const USER_CONTROL_DIR_PATH: &[&str] = &["home", "1000", "control"];
pub const SHARED_PROJECT_A_DEMO_CLAIM_PATH: &[&str] =
    &["shared", "project-a", "collab", "task", "demo", "claim"];
pub const SHARED_PROJECT_A_LOCK_LEASE_PATH: &[&str] =
    &["shared", "project-a", "collab", "lock", "lease"];
pub const POSTGRES_DSN_DIR_PATH: &[&str] = &["db", "postgres", "dsn"];
pub const USER_MODELS_DIR_PATH: &[&str] = &["home", "1000", "model"];
pub const SHELL_EXEC_TOOL_INBOX_PATH: &[&str] = &["tool", "shell.exec", "invoke", "inbox"];
pub const SHELL_EXEC_TOOL_OUTBOX_PATH: &[&str] = &["tool", "shell.exec", "invoke", "outbox"];
pub const FILESYSTEM_READ_TOOL_INBOX_PATH: &[&str] =
    &["tool", "filesystem.read", "invoke", "inbox"];
pub const FILESYSTEM_READ_TOOL_OUTBOX_PATH: &[&str] =
    &["tool", "filesystem.read", "invoke", "outbox"];
pub const MCP_LOCAL_FS_READ_TOOL_INBOX_PATH: &[&str] =
    &["tool", "mcp.local-fs.read_file", "invoke", "inbox"];
pub const MCP_LOCAL_FS_READ_TOOL_OUTBOX_PATH: &[&str] =
    &["tool", "mcp.local-fs.read_file", "invoke", "outbox"];
pub const MCP_SUMMARIZE_PROMPT_RENDER_INBOX_PATH: &[&str] = &[
    "mcp",
    "prompt",
    "local-fs",
    "summarize-file",
    "render",
    "inbox",
];
pub const MCP_SUMMARIZE_PROMPT_RENDER_OUTBOX_PATH: &[&str] = &[
    "mcp",
    "prompt",
    "local-fs",
    "summarize-file",
    "render",
    "outbox",
];
pub const MCP_SESSION_DIR_PATH: &[&str] = &["mcp", "session", "local-fs.demo"];
pub const MCP_SESSION_SEARCH_DIR_PATH: &[&str] = &["mcp", "session", "local-fs.demo", "search"];
pub const AGENT_HELPER_INBOX_PATH: &[&str] = &["agent", "helper", "inbox"];
pub const AGENT_HELPER_OUTBOX_PATH: &[&str] = &["agent", "helper", "outbox"];
pub const AGENT_HELPER_CONTROL_PATH: &[&str] = &["agent", "helper", "control"];
pub const CLUSTER_TASK_PENDING_PATH: &[&str] = &["cluster", "local", "queue", "default", "pending"];
pub const CLUSTER_TASK_DONE_PATH: &[&str] = &["cluster", "local", "queue", "default", "done"];
pub const CLUSTER_TASK_FAILED_PATH: &[&str] = &["cluster", "local", "queue", "default", "failed"];
pub const CLUSTER_TASKS_PATH: &[&str] = &["cluster", "local", "task"];
pub const CLUSTER_LOCAL_CONTROL_PATH: &[&str] = &["cluster", "local", "control"];
pub const DEFAULT_THREAD_FORMAT: &str = "openai.chat";
pub const TOOL_FORMAT: &str = "tool.invoke";
pub const CLUSTER_TASK_FORMAT: &str = "cluster.task";
pub const MEMORY_WORKING_FORMAT: &str = "memory.working";
pub const MEMORY_EPISODIC_FORMAT: &str = "memory.episodic";
pub const MEMORY_SEMANTIC_FORMAT: &str = "memory.semantic";
pub const MEMORY_PROCEDURAL_FORMAT: &str = "memory.procedural";
pub const MEMORY_PROFILE_FORMAT: &str = "memory.profile";
pub const PREFERENCE_PAIR_FORMAT: &str = "feedback.preference";
pub const MCP_PROMPT_RENDER_FORMAT: &str = "mcp.prompt.render";
pub const AGENT_TASK_FORMAT: &str = "agent.task";
pub const SHELL_EXEC_TOOL: &str = cortex_tools::SHELL_EXEC_TOOL;
pub const FILESYSTEM_READ_TOOL: &str = cortex_tools::FILESYSTEM_READ_TOOL;
pub const MCP_LOCAL_FS_READ_TOOL: &str = cortex_tools::MCP_LOCAL_FS_READ_TOOL;
pub const CORTEX_CONTEXT_XATTR: &str = "user.cortex.context";
pub const CORTEX_CONTEXT_XATTR_LIST: &[u8] = b"user.cortex.context\0";
