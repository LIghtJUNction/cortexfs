use crate::{
    AGENT_HELPER_INBOX_PATH, AGENT_HELPER_OUTBOX_PATH, AGENT_TASK_FORMAT, API_FORMATS, API_PREFIX,
    BATCH_INBOX_PATH, BATCH_OUTBOX_PATH, CLUSTER_TASK_FORMAT, CLUSTER_TASK_PENDING_PATH,
    DEFAULT_BATCH_FORMAT, DEFAULT_THREAD_FORMAT, DEMO_THREAD_INBOX_PATH,
    EXTERNAL_QQ_GROUP_THREAD_COMPAT_INBOX_PATH, EXTERNAL_QQ_GROUP_THREAD_INBOX_PATH,
    FEEDBACK_PREFERENCE_INBOX_PATH, FEEDBACK_PREFERENCE_OUTBOX_PATH, FILESYSTEM_READ_TOOL,
    FILESYSTEM_READ_TOOL_INBOX_PATH, FILESYSTEM_READ_TOOL_OUTBOX_PATH, LOCAL_USER_HOME_PREFIX,
    LOCAL_USER_ID, MCP_LOCAL_FS_READ_TOOL, MCP_LOCAL_FS_READ_TOOL_INBOX_PATH,
    MCP_LOCAL_FS_READ_TOOL_OUTBOX_PATH, MCP_PROMPT_RENDER_FORMAT,
    MCP_SUMMARIZE_PROMPT_RENDER_INBOX_PATH, MCP_SUMMARIZE_PROMPT_RENDER_OUTBOX_PATH,
    MEMORY_ITEM_FORMAT, MEMORY_SEMANTIC_INBOX_PATH, PREFERENCE_PAIR_FORMAT,
    SHARED_PROJECT_A_COMPAT_DEMO_CLAIMS_PATH, SHARED_PROJECT_A_COMPAT_LOCK_LEASES_PATH,
    SHARED_PROJECT_A_DEMO_CLAIMS_PATH, SHARED_PROJECT_A_LOCK_LEASES_PATH, TOOL_FORMAT,
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SubmissionDirectoryKind {
    Inbox,
    Outbox,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum SubmissionScope {
    Api,
    Batch,
    Thread,
    ExternalThread,
    Tool,
    AgentTask,
    ClusterTask,
    MemoryItem,
    PreferencePair,
    McpPromptRender,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct SubmissionLocation {
    pub scope: SubmissionScope,
    pub format: &'static str,
    pub tool: Option<&'static str>,
    pub kind: SubmissionDirectoryKind,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct CollabClaimLocation {
    pub task_id: &'static str,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct CollabLockLocation {
    pub scope: &'static str,
}

impl SubmissionLocation {
    pub fn from_path(path: &[String]) -> Option<Self> {
        let canonical_path;
        let path = if is_local_user_space_compat_path(path) {
            canonical_path = canonical_user_home_path(path)?;
            canonical_path.as_slice()
        } else {
            path
        };
        Self::from_api_path(path)
            .or_else(|| Self::from_batch_path(path))
            .or_else(|| Self::from_thread_path(path))
            .or_else(|| Self::from_external_thread_path(path))
            .or_else(|| Self::from_tool_path(path))
            .or_else(|| Self::from_agent_task_path(path))
            .or_else(|| Self::from_cluster_task_path(path))
            .or_else(|| Self::from_memory_item_path(path))
            .or_else(|| Self::from_preference_path(path))
            .or_else(|| Self::from_mcp_prompt_render_path(path))
    }

    fn from_api_path(path: &[String]) -> Option<Self> {
        if path.len() != API_PREFIX.len().saturating_add(2)
            || !path
                .iter()
                .zip(API_PREFIX)
                .all(|(actual, expected)| actual == expected)
        {
            return None;
        }
        let format_name = path.get(API_PREFIX.len())?;
        let kind_name = path.get(API_PREFIX.len().saturating_add(1))?;
        let format = API_FORMATS
            .iter()
            .copied()
            .find(|format| format_name == *format)?;
        let kind = Self::kind_from_name(kind_name)?;
        Some(Self {
            scope: SubmissionScope::Api,
            format,
            tool: None,
            kind,
        })
    }

    fn from_batch_path(path: &[String]) -> Option<Self> {
        if path.len() == BATCH_INBOX_PATH.len()
            && path
                .iter()
                .zip(BATCH_INBOX_PATH)
                .all(|(actual, expected)| actual == expected)
        {
            return Some(Self {
                scope: SubmissionScope::Batch,
                format: DEFAULT_BATCH_FORMAT,
                tool: None,
                kind: SubmissionDirectoryKind::Inbox,
            });
        }
        if path.len() == BATCH_OUTBOX_PATH.len()
            && path
                .iter()
                .zip(BATCH_OUTBOX_PATH)
                .all(|(actual, expected)| actual == expected)
        {
            return Some(Self {
                scope: SubmissionScope::Batch,
                format: DEFAULT_BATCH_FORMAT,
                tool: None,
                kind: SubmissionDirectoryKind::Outbox,
            });
        }
        None
    }

    fn from_thread_path(path: &[String]) -> Option<Self> {
        if path.len() == DEMO_THREAD_INBOX_PATH.len()
            && path
                .iter()
                .zip(DEMO_THREAD_INBOX_PATH)
                .all(|(actual, expected)| actual == expected)
        {
            return Some(Self {
                scope: SubmissionScope::Thread,
                format: DEFAULT_THREAD_FORMAT,
                tool: None,
                kind: SubmissionDirectoryKind::Inbox,
            });
        }
        None
    }

    fn from_external_thread_path(path: &[String]) -> Option<Self> {
        if path_eq_any(
            path,
            &[
                EXTERNAL_QQ_GROUP_THREAD_INBOX_PATH,
                EXTERNAL_QQ_GROUP_THREAD_COMPAT_INBOX_PATH,
            ],
        ) {
            return Some(Self {
                scope: SubmissionScope::ExternalThread,
                format: DEFAULT_THREAD_FORMAT,
                tool: None,
                kind: SubmissionDirectoryKind::Inbox,
            });
        }
        None
    }

    fn from_tool_path(path: &[String]) -> Option<Self> {
        [
            (
                FILESYSTEM_READ_TOOL,
                FILESYSTEM_READ_TOOL_INBOX_PATH,
                SubmissionDirectoryKind::Inbox,
            ),
            (
                FILESYSTEM_READ_TOOL,
                FILESYSTEM_READ_TOOL_OUTBOX_PATH,
                SubmissionDirectoryKind::Outbox,
            ),
            (
                MCP_LOCAL_FS_READ_TOOL,
                MCP_LOCAL_FS_READ_TOOL_INBOX_PATH,
                SubmissionDirectoryKind::Inbox,
            ),
            (
                MCP_LOCAL_FS_READ_TOOL,
                MCP_LOCAL_FS_READ_TOOL_OUTBOX_PATH,
                SubmissionDirectoryKind::Outbox,
            ),
        ]
        .into_iter()
        .find_map(|(tool, expected_path, kind)| {
            (path.len() == expected_path.len()
                && path
                    .iter()
                    .zip(expected_path)
                    .all(|(actual, expected)| actual == expected))
            .then_some(Self {
                scope: SubmissionScope::Tool,
                format: TOOL_FORMAT,
                tool: Some(tool),
                kind,
            })
        })
    }

    fn from_cluster_task_path(path: &[String]) -> Option<Self> {
        if path.len() == CLUSTER_TASK_PENDING_PATH.len()
            && path
                .iter()
                .zip(CLUSTER_TASK_PENDING_PATH)
                .all(|(actual, expected)| actual == expected)
        {
            return Some(Self {
                scope: SubmissionScope::ClusterTask,
                format: CLUSTER_TASK_FORMAT,
                tool: None,
                kind: SubmissionDirectoryKind::Inbox,
            });
        }
        None
    }

    fn from_agent_task_path(path: &[String]) -> Option<Self> {
        if path.len() == AGENT_HELPER_INBOX_PATH.len()
            && path
                .iter()
                .zip(AGENT_HELPER_INBOX_PATH)
                .all(|(actual, expected)| actual == expected)
        {
            return Some(Self {
                scope: SubmissionScope::AgentTask,
                format: AGENT_TASK_FORMAT,
                tool: None,
                kind: SubmissionDirectoryKind::Inbox,
            });
        }
        if path.len() == AGENT_HELPER_OUTBOX_PATH.len()
            && path
                .iter()
                .zip(AGENT_HELPER_OUTBOX_PATH)
                .all(|(actual, expected)| actual == expected)
        {
            return Some(Self {
                scope: SubmissionScope::AgentTask,
                format: AGENT_TASK_FORMAT,
                tool: None,
                kind: SubmissionDirectoryKind::Outbox,
            });
        }
        None
    }

    fn from_memory_item_path(path: &[String]) -> Option<Self> {
        if path.len() == MEMORY_SEMANTIC_INBOX_PATH.len()
            && path
                .iter()
                .zip(MEMORY_SEMANTIC_INBOX_PATH)
                .all(|(actual, expected)| actual == expected)
        {
            return Some(Self {
                scope: SubmissionScope::MemoryItem,
                format: MEMORY_ITEM_FORMAT,
                tool: None,
                kind: SubmissionDirectoryKind::Inbox,
            });
        }
        None
    }

    fn from_preference_path(path: &[String]) -> Option<Self> {
        if path.len() == FEEDBACK_PREFERENCE_INBOX_PATH.len()
            && path
                .iter()
                .zip(FEEDBACK_PREFERENCE_INBOX_PATH)
                .all(|(actual, expected)| actual == expected)
        {
            return Some(Self {
                scope: SubmissionScope::PreferencePair,
                format: PREFERENCE_PAIR_FORMAT,
                tool: None,
                kind: SubmissionDirectoryKind::Inbox,
            });
        }
        if path.len() == FEEDBACK_PREFERENCE_OUTBOX_PATH.len()
            && path
                .iter()
                .zip(FEEDBACK_PREFERENCE_OUTBOX_PATH)
                .all(|(actual, expected)| actual == expected)
        {
            return Some(Self {
                scope: SubmissionScope::PreferencePair,
                format: PREFERENCE_PAIR_FORMAT,
                tool: None,
                kind: SubmissionDirectoryKind::Outbox,
            });
        }
        None
    }

    fn from_mcp_prompt_render_path(path: &[String]) -> Option<Self> {
        if path.len() == MCP_SUMMARIZE_PROMPT_RENDER_INBOX_PATH.len()
            && path
                .iter()
                .zip(MCP_SUMMARIZE_PROMPT_RENDER_INBOX_PATH)
                .all(|(actual, expected)| actual == expected)
        {
            return Some(Self {
                scope: SubmissionScope::McpPromptRender,
                format: MCP_PROMPT_RENDER_FORMAT,
                tool: None,
                kind: SubmissionDirectoryKind::Inbox,
            });
        }
        if path.len() == MCP_SUMMARIZE_PROMPT_RENDER_OUTBOX_PATH.len()
            && path
                .iter()
                .zip(MCP_SUMMARIZE_PROMPT_RENDER_OUTBOX_PATH)
                .all(|(actual, expected)| actual == expected)
        {
            return Some(Self {
                scope: SubmissionScope::McpPromptRender,
                format: MCP_PROMPT_RENDER_FORMAT,
                tool: None,
                kind: SubmissionDirectoryKind::Outbox,
            });
        }
        None
    }

    fn kind_from_name(name: &str) -> Option<SubmissionDirectoryKind> {
        match name {
            "inbox" => Some(SubmissionDirectoryKind::Inbox),
            "outbox" => Some(SubmissionDirectoryKind::Outbox),
            _ => None,
        }
    }
}

fn is_local_user_space_compat_path(path: &[String]) -> bool {
    matches!(
        path,
        [root, users, uid, ..]
            if (root == "space" || root == "spaces") && users == "users" && uid == LOCAL_USER_ID
    )
}

fn canonical_user_home_path(path: &[String]) -> Option<Vec<String>> {
    if !is_local_user_space_compat_path(path) {
        return None;
    }
    let mut canonical = Vec::with_capacity(path.len().saturating_sub(1));
    canonical.extend(LOCAL_USER_HOME_PREFIX.iter().map(ToString::to_string));
    canonical.extend(path.iter().skip(3).cloned());
    Some(canonical)
}

impl CollabClaimLocation {
    pub fn from_path(path: &[String]) -> Option<Self> {
        if path_eq_any(
            path,
            &[
                SHARED_PROJECT_A_DEMO_CLAIMS_PATH,
                SHARED_PROJECT_A_COMPAT_DEMO_CLAIMS_PATH,
            ],
        ) {
            return Some(Self { task_id: "demo" });
        }
        None
    }
}

impl CollabLockLocation {
    pub fn from_path(path: &[String]) -> Option<Self> {
        if path_eq_any(
            path,
            &[
                SHARED_PROJECT_A_LOCK_LEASES_PATH,
                SHARED_PROJECT_A_COMPAT_LOCK_LEASES_PATH,
            ],
        ) {
            return Some(Self { scope: "shared" });
        }
        None
    }
}

fn path_eq_any(path: &[String], expected_paths: &[&[&str]]) -> bool {
    expected_paths.iter().any(|expected_path| {
        path.len() == expected_path.len()
            && path
                .iter()
                .zip(*expected_path)
                .all(|(actual, expected)| actual == expected)
    })
}
