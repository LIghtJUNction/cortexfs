use std::collections::BTreeMap;

use fuse3::Inode;

use crate::abi::{
    AGENT_HELPER_CONTROL_PATH, AGENT_HELPER_OUTBOX_PATH, AUDIT_DIR_PATH, BATCH_DIR_PATH,
    CLUSTER_LOCAL_CONTROL_PATH, CLUSTER_LOCAL_STATE_PATH, CLUSTER_TASK_DONE_PATH,
    CLUSTER_TASKS_PATH, CONTROL_DIR_PATH, DEMO_THREAD_CONTROL_PATH, DEMO_THREAD_DIR_PATH,
    DEMO_THREAD_TOOL_LOOP_CONTROL_PATH, DEMO_THREAD_TOOL_LOOP_LIMITS_PATH,
    DEMO_THREAD_TOOL_LOOP_PATH, DEMO_THREAD_TOOL_LOOP_STATE_PATH, EXPORT_DIR_PATH,
    EXPORT_FILTERS_DIR_PATH, EXTERNAL_QQ_GROUP_THREAD_DIR_PATH,
    EXTERNAL_QQ_SUBJECT_QUOTA_REQUESTS_PATH, FEEDBACK_PREFERENCE_OUTBOX_PATH,
    MCP_SUMMARIZE_PROMPT_RENDER_OUTBOX_PATH, MEMORY_SEARCH_DIR_PATH, MEMORY_SEMANTIC_DIR_PATH,
    POSTGRES_DSN_DIR_PATH, ROOT_INODE, USER_CONTROL_DIR_PATH, USER_MODELS_DIR_PATH,
    USER_POLICY_DIR_PATH, USER_ROUTES_DIR_PATH,
};
use crate::providers::{PROVIDER_SPECS, provider_child_path, provider_model_id, user_model_path};
use crate::runtime_types::ProviderRuntimeParents;
use crate::tree::StaticTree;

#[derive(Debug, Clone, Copy)]
pub struct McpRuntimeParents {
    pub local_fs_status: Option<Inode>,
    pub local_fs_pid: Option<Inode>,
    pub local_fs_control: Option<Inode>,
    pub workspace_content: Option<Inode>,
    pub workspace_refresh: Option<Inode>,
    pub session_state: Option<Inode>,
    pub session_transcript: Option<Inode>,
}

#[derive(Debug, Clone)]
pub struct RuntimeParents {
    pub audit: Inode,
    pub audit_cost: Option<Inode>,
    pub control: Inode,
    pub batch: Option<Inode>,
    pub exports: Option<Inode>,
    pub exports_compat: Option<Inode>,
    pub export_filters: Option<Inode>,
    pub export_filters_compat: Option<Inode>,
    pub feedback_preference_outbox: Option<Inode>,
    pub thread: Option<Inode>,
    pub thread_control: Option<Inode>,
    pub external_thread: Option<Inode>,
    pub external_subject_quota_requests: Option<Inode>,
    pub tool_loop: Option<Inode>,
    pub tool_loop_state: Option<Inode>,
    pub tool_loop_control: Option<Inode>,
    pub tool_loop_limits: Option<Inode>,
    pub memory_working: Option<Inode>,
    pub memory_episodic: Option<Inode>,
    pub memory_search: Option<Inode>,
    pub memory_semantic: Option<Inode>,
    pub memory_procedural: Option<Inode>,
    pub memory_profile: Option<Inode>,
    pub mcp_local_fs_status: Option<Inode>,
    pub mcp_local_fs_pid: Option<Inode>,
    pub mcp_local_fs_control: Option<Inode>,
    pub mcp_workspace_content: Option<Inode>,
    pub mcp_workspace_refresh: Option<Inode>,
    pub mcp_session_state: Option<Inode>,
    pub mcp_session_transcript: Option<Inode>,
    pub agent_helper_outbox: Option<Inode>,
    pub agent_helper_control: Option<Inode>,
    pub agent_helper_runtime_state: Option<Inode>,
    pub agent_helper_runtime_pid: Option<Inode>,
    pub agent_helper_runtime_heartbeat: Option<Inode>,
    pub agent_helper_runtime_current_thread: Option<Inode>,
    pub agent_helper_runtime_current_task: Option<Inode>,
    pub collab_task_owner: Option<Inode>,
    pub collab_task_state: Option<Inode>,
    pub collab_task_events: Option<Inode>,
    pub collab_locks: Option<Inode>,
    pub mcp_prompt_render_outbox: Option<Inode>,
    pub user_policy: Option<Inode>,
    pub user_routes: Option<Inode>,
    pub user_routes_compat: Option<Inode>,
    pub user_control: Option<Inode>,
    pub user_models: Option<Inode>,
    pub user_models_compat: Option<Inode>,
    pub user_models_by_provider: BTreeMap<&'static str, Inode>,
    pub user_models_compat_by_provider: BTreeMap<&'static str, Inode>,
    pub cluster_state: Option<Inode>,
    pub cluster_worker_state: Option<Inode>,
    pub cluster_worker_heartbeat: Option<Inode>,
    pub cluster_worker_load: Option<Inode>,
    pub cluster_worker_current_task: Option<Inode>,
    pub cluster_control: Option<Inode>,
    pub cluster_tasks: Option<Inode>,
    pub cluster_done: Option<Inode>,
    pub pgvector_enabled: Option<Inode>,
    pub pgvector_status: Option<Inode>,
    pub pgvector_collections: Option<Inode>,
    pub pgvector_refresh: Option<Inode>,
    pub postgres_status: Option<Inode>,
    pub postgres_dsn: Option<Inode>,
    pub provider_parents: BTreeMap<&'static str, ProviderRuntimeParents>,
}

#[derive(Debug, Clone, Copy)]
struct ClusterWorkerRuntimeParents {
    state: Option<Inode>,
    heartbeat: Option<Inode>,
    load: Option<Inode>,
    current_task: Option<Inode>,
}

#[derive(Debug, Clone, Copy)]
struct AgentRuntimeParents {
    state: Option<Inode>,
    pid: Option<Inode>,
    heartbeat: Option<Inode>,
    current_thread: Option<Inode>,
    current_task: Option<Inode>,
}

impl RuntimeParents {
    pub fn from_tree(tree: &StaticTree) -> Self {
        let mcp = mcp_runtime_parents(tree);
        let agent = agent_runtime_parents(tree);
        let cluster_worker = cluster_worker_runtime_parents(tree);
        Self {
            audit: tree.path_inode(AUDIT_DIR_PATH).unwrap_or(ROOT_INODE),
            audit_cost: tree.path_inode(&["audit", "cost"]),
            control: tree.path_inode(CONTROL_DIR_PATH).unwrap_or(ROOT_INODE),
            batch: tree.path_inode(BATCH_DIR_PATH),
            exports: tree.path_inode(EXPORT_DIR_PATH),
            exports_compat: tree.path_inode(&["home", "1000", "exports"]),
            export_filters: tree.path_inode(EXPORT_FILTERS_DIR_PATH),
            export_filters_compat: tree.path_inode(&["home", "1000", "exports", "filters"]),
            feedback_preference_outbox: tree.path_inode(FEEDBACK_PREFERENCE_OUTBOX_PATH),
            thread: tree.path_inode(DEMO_THREAD_DIR_PATH),
            thread_control: tree.path_inode(DEMO_THREAD_CONTROL_PATH),
            external_thread: tree.path_inode(EXTERNAL_QQ_GROUP_THREAD_DIR_PATH),
            external_subject_quota_requests: tree
                .path_inode(EXTERNAL_QQ_SUBJECT_QUOTA_REQUESTS_PATH),
            tool_loop: tree.path_inode(DEMO_THREAD_TOOL_LOOP_PATH),
            tool_loop_state: tree.path_inode(DEMO_THREAD_TOOL_LOOP_STATE_PATH),
            tool_loop_control: tree.path_inode(DEMO_THREAD_TOOL_LOOP_CONTROL_PATH),
            tool_loop_limits: tree.path_inode(DEMO_THREAD_TOOL_LOOP_LIMITS_PATH),
            memory_working: tree.path_inode(&["home", "1000", "memory", "working"]),
            memory_episodic: tree.path_inode(&["home", "1000", "memory", "episodic"]),
            memory_search: tree.path_inode(MEMORY_SEARCH_DIR_PATH),
            memory_semantic: tree.path_inode(MEMORY_SEMANTIC_DIR_PATH),
            memory_procedural: tree.path_inode(&["home", "1000", "memory", "procedural"]),
            memory_profile: tree.path_inode(&["home", "1000", "memory", "profile"]),
            mcp_local_fs_status: mcp.local_fs_status,
            mcp_local_fs_pid: mcp.local_fs_pid,
            mcp_local_fs_control: mcp.local_fs_control,
            mcp_workspace_content: mcp.workspace_content,
            mcp_workspace_refresh: mcp.workspace_refresh,
            mcp_session_state: mcp.session_state,
            mcp_session_transcript: mcp.session_transcript,
            agent_helper_outbox: tree.path_inode(AGENT_HELPER_OUTBOX_PATH),
            agent_helper_control: tree.path_inode(AGENT_HELPER_CONTROL_PATH),
            agent_helper_runtime_state: agent.state,
            agent_helper_runtime_pid: agent.pid,
            agent_helper_runtime_heartbeat: agent.heartbeat,
            agent_helper_runtime_current_thread: agent.current_thread,
            agent_helper_runtime_current_task: agent.current_task,
            collab_task_owner: tree.path_inode(&[
                "spaces",
                "shared",
                "project-a",
                "collab",
                "task",
                "demo",
                "owner",
            ]),
            collab_task_state: tree.path_inode(&[
                "spaces",
                "shared",
                "project-a",
                "collab",
                "task",
                "demo",
                "state",
            ]),
            collab_task_events: tree.path_inode(&[
                "spaces",
                "shared",
                "project-a",
                "collab",
                "task",
                "demo",
                "events.jsonl",
            ]),
            collab_locks: tree.path_inode(&["spaces", "shared", "project-a", "collab", "lock"]),
            mcp_prompt_render_outbox: tree.path_inode(MCP_SUMMARIZE_PROMPT_RENDER_OUTBOX_PATH),
            user_policy: tree.path_inode(USER_POLICY_DIR_PATH),
            user_routes: tree.path_inode(USER_ROUTES_DIR_PATH),
            user_routes_compat: tree.path_inode(&["home", "1000", "routes"]),
            user_control: tree.path_inode(USER_CONTROL_DIR_PATH),
            user_models: tree.path_inode(USER_MODELS_DIR_PATH),
            user_models_compat: tree.path_inode(&["home", "1000", "models"]),
            user_models_by_provider: user_model_parents(tree),
            user_models_compat_by_provider: user_model_compat_parents(tree),
            cluster_state: tree.path_inode(CLUSTER_LOCAL_STATE_PATH),
            cluster_worker_state: cluster_worker.state,
            cluster_worker_heartbeat: cluster_worker.heartbeat,
            cluster_worker_load: cluster_worker.load,
            cluster_worker_current_task: cluster_worker.current_task,
            cluster_control: tree.path_inode(CLUSTER_LOCAL_CONTROL_PATH),
            cluster_tasks: tree.path_inode(CLUSTER_TASKS_PATH),
            cluster_done: tree.path_inode(CLUSTER_TASK_DONE_PATH),
            pgvector_enabled: tree.path_inode(&["vector", "store", "pgvector", "enabled"]),
            pgvector_status: tree.path_inode(&["vector", "store", "pgvector", "status"]),
            pgvector_collections: tree.path_inode(&["vector", "store", "pgvector", "collections"]),
            pgvector_refresh: tree.path_inode(&["vector", "store", "pgvector", "refresh"]),
            postgres_status: tree.path_inode(&["db", "postgres", "status"]),
            postgres_dsn: tree.path_inode(POSTGRES_DSN_DIR_PATH),
            provider_parents: provider_runtime_parents(tree),
        }
    }
}

fn user_model_parents(tree: &StaticTree) -> BTreeMap<&'static str, Inode> {
    PROVIDER_SPECS
        .iter()
        .filter_map(|provider| {
            tree.path_inode_owned(&user_model_path(provider))
                .map(|inode| (provider.id, inode))
        })
        .collect()
}

fn user_model_compat_parents(tree: &StaticTree) -> BTreeMap<&'static str, Inode> {
    PROVIDER_SPECS
        .iter()
        .filter_map(|provider| {
            let path = vec![
                "home".to_owned(),
                "1000".to_owned(),
                "models".to_owned(),
                provider_model_id(provider),
            ];
            tree.path_inode_owned(&path)
                .map(|inode| (provider.id, inode))
        })
        .collect()
}

fn provider_runtime_parents(tree: &StaticTree) -> BTreeMap<&'static str, ProviderRuntimeParents> {
    PROVIDER_SPECS
        .iter()
        .filter_map(|provider| {
            let url = tree.path_inode_owned(&provider_child_path(provider.id, "url"))?;
            let url_compat = tree.path_inode_owned(&provider_child_path(provider.id, "base_url"));
            let enabled = tree.path_inode_owned(&provider_child_path(provider.id, "enabled"))?;
            let health = tree.path_inode_owned(&provider_child_path(provider.id, "health"))?;
            let models = tree.path_inode_owned(&provider_child_path(provider.id, "model"))?;
            let models_compat = tree.path_inode_owned(&provider_child_path(provider.id, "models"));
            let secrets = tree.path_inode_owned(&provider_child_path(provider.id, "secrets"))?;
            Some((
                provider.id,
                ProviderRuntimeParents {
                    url,
                    url_compat,
                    enabled,
                    health,
                    models,
                    models_compat,
                    secrets,
                },
            ))
        })
        .collect()
}

fn agent_runtime_parents(tree: &StaticTree) -> AgentRuntimeParents {
    AgentRuntimeParents {
        state: tree.path_inode(&["agent", "helper", "runtime", "state"]),
        pid: tree.path_inode(&["agent", "helper", "runtime", "pid"]),
        heartbeat: tree.path_inode(&["agent", "helper", "runtime", "heartbeat"]),
        current_thread: tree.path_inode(&["agent", "helper", "runtime", "current_thread"]),
        current_task: tree.path_inode(&["agent", "helper", "runtime", "current_task"]),
    }
}

fn cluster_worker_runtime_parents(tree: &StaticTree) -> ClusterWorkerRuntimeParents {
    ClusterWorkerRuntimeParents {
        state: tree.path_inode(&["cluster", "local", "worker", "local-worker", "state"]),
        heartbeat: tree.path_inode(&["cluster", "local", "worker", "local-worker", "heartbeat"]),
        load: tree.path_inode(&["cluster", "local", "worker", "local-worker", "load"]),
        current_task: tree.path_inode(&[
            "cluster",
            "local",
            "worker",
            "local-worker",
            "current_task",
        ]),
    }
}

fn mcp_runtime_parents(tree: &StaticTree) -> McpRuntimeParents {
    McpRuntimeParents {
        local_fs_status: tree.path_inode(&["mcp", "server", "local-fs", "status"]),
        local_fs_pid: tree.path_inode(&["mcp", "server", "local-fs", "pid"]),
        local_fs_control: tree.path_inode(&["mcp", "server", "local-fs", "control"]),
        workspace_content: tree.path_inode(&[
            "mcp",
            "resource",
            "local-fs",
            "workspace",
            "content",
        ]),
        workspace_refresh: tree.path_inode(&[
            "mcp",
            "resource",
            "local-fs",
            "workspace",
            "refresh",
        ]),
        session_state: tree.path_inode(&["mcp", "session", "local-fs.demo", "state"]),
        session_transcript: tree.path_inode(&[
            "mcp",
            "session",
            "local-fs.demo",
            "transcript.jsonl",
        ]),
    }
}
