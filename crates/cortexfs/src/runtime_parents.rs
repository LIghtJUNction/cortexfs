use std::collections::BTreeMap;

use fuse3::Inode;

use crate::abi::{
    AGENT_HELPER_CONTROL_PATH, AGENT_HELPER_OUTBOX_PATH, AUDIT_DIR_PATH, BATCH_DIR_PATH,
    CLUSTER_LOCAL_CONTROL_PATH, CLUSTER_TASK_DONE_PATH, CLUSTER_TASK_FAILED_PATH,
    CLUSTER_TASK_PENDING_PATH, CLUSTER_TASKS_PATH, CONTROL_DIR_PATH, DEMO_THREAD_CONTROL_PATH,
    DEMO_THREAD_DIR_PATH, DEMO_THREAD_TOOL_LOOP_CONTROL_PATH, DEMO_THREAD_TOOL_LOOP_LIMITS_PATH,
    DEMO_THREAD_TOOL_LOOP_PATH, EXPORT_DIR_PATH, EXPORT_FILTERS_DIR_PATH,
    EXTERNAL_QQ_GROUP_THREAD_DIR_PATH, EXTERNAL_QQ_SUBJECT_QUOTA_DIR_PATH,
    FEEDBACK_PREFERENCE_OUTBOX_PATH, MCP_SUMMARIZE_PROMPT_RENDER_OUTBOX_PATH,
    MEMORY_SEARCH_DIR_PATH, MEMORY_SEMANTIC_DIR_PATH, POSTGRES_DSN_DIR_PATH, ROOT_INODE,
    USER_CONTROL_DIR_PATH, USER_MODELS_DIR_PATH, USER_POLICY_DIR_PATH, USER_ROUTES_DIR_PATH,
};
use crate::providers::{PROVIDER_SPECS, provider_child_path, user_model_path};
use crate::runtime_types::ProviderRuntimeParents;
use crate::tree::StaticTree;

#[derive(Debug, Clone, Copy)]
pub struct McpRuntimeParents {
    pub local_fs_server: Option<Inode>,
    pub local_fs_control: Option<Inode>,
    pub workspace: Option<Inode>,
    pub session: Option<Inode>,
}

#[derive(Debug, Clone)]
pub struct RuntimeParents {
    pub audit: Inode,
    pub audit_cost: Option<Inode>,
    pub control: Inode,
    pub batch: Option<Inode>,
    pub exports: Option<Inode>,
    pub export_filters: Option<Inode>,
    pub convert: Option<Inode>,
    pub cache: Option<Inode>,
    pub user_audit: Option<Inode>,
    pub local_api: Option<Inode>,
    pub local_api_http: Option<Inode>,
    pub local_api_unix: Option<Inode>,
    pub feedback_preference_outbox: Option<Inode>,
    pub thread: Option<Inode>,
    pub thread_control: Option<Inode>,
    pub external_thread: Option<Inode>,
    pub external_subject_quota: Option<Inode>,
    pub tool_loop: Option<Inode>,
    pub tool_loop_control: Option<Inode>,
    pub tool_loop_limits: Option<Inode>,
    pub memory_working: Option<Inode>,
    pub memory_episodic: Option<Inode>,
    pub memory_search: Option<Inode>,
    pub memory_semantic: Option<Inode>,
    pub memory_procedural: Option<Inode>,
    pub memory_profile: Option<Inode>,
    pub memory_index: Option<Inode>,
    pub mcp_local_fs_server: Option<Inode>,
    pub mcp_local_fs_control: Option<Inode>,
    pub mcp_workspace: Option<Inode>,
    pub mcp_session: Option<Inode>,
    pub installed_skill_cortexfs_test: Option<Inode>,
    pub agent_helper_outbox: Option<Inode>,
    pub agent_helper_control: Option<Inode>,
    pub agent_helper_runtime: Option<Inode>,
    pub collab_blackboard: Option<Inode>,
    pub collab_task_demo: Option<Inode>,
    pub collab_handoff_demo: Option<Inode>,
    pub collab_lock_demo: Option<Inode>,
    pub collab_locks: Option<Inode>,
    pub mcp_prompt_render_outbox: Option<Inode>,
    pub user_policy: Option<Inode>,
    pub user_routes: Option<Inode>,
    pub user_control: Option<Inode>,
    pub user_models: Option<Inode>,
    pub user_models_by_provider: BTreeMap<&'static str, Inode>,
    pub cluster_local: Option<Inode>,
    pub cluster_worker: Option<Inode>,
    pub cluster_control: Option<Inode>,
    pub cluster_pending: Option<Inode>,
    pub cluster_running: Option<Inode>,
    pub cluster_tasks: Option<Inode>,
    pub cluster_done: Option<Inode>,
    pub cluster_failed: Option<Inode>,
    pub vector_stores: BTreeMap<&'static str, Inode>,
    pub pgvector_store: Option<Inode>,
    pub sqlite: Option<Inode>,
    pub postgres: Option<Inode>,
    pub postgres_dsn: Option<Inode>,
    pub provider_parents: BTreeMap<&'static str, ProviderRuntimeParents>,
}

impl RuntimeParents {
    pub fn from_tree(tree: &StaticTree) -> Self {
        let mcp = mcp_runtime_parents(tree);
        Self {
            audit: tree.path_inode(AUDIT_DIR_PATH).unwrap_or(ROOT_INODE),
            audit_cost: tree.path_inode(&["audit", "cost"]),
            control: tree.path_inode(CONTROL_DIR_PATH).unwrap_or(ROOT_INODE),
            batch: tree.path_inode(BATCH_DIR_PATH),
            exports: tree.path_inode(EXPORT_DIR_PATH),
            export_filters: tree.path_inode(EXPORT_FILTERS_DIR_PATH),
            convert: tree.path_inode(&["home", "1000", "convert"]),
            cache: tree.path_inode(&["home", "1000", "cache"]),
            user_audit: tree.path_inode(&["home", "1000", "audit"]),
            local_api: tree.path_inode(&["home", "1000", "api"]),
            local_api_http: tree.path_inode(&["home", "1000", "api", "http"]),
            local_api_unix: tree.path_inode(&["home", "1000", "api", "unix"]),
            feedback_preference_outbox: tree.path_inode(FEEDBACK_PREFERENCE_OUTBOX_PATH),
            thread: tree.path_inode(DEMO_THREAD_DIR_PATH),
            thread_control: tree.path_inode(DEMO_THREAD_CONTROL_PATH),
            external_thread: tree.path_inode(EXTERNAL_QQ_GROUP_THREAD_DIR_PATH),
            external_subject_quota: tree.path_inode(EXTERNAL_QQ_SUBJECT_QUOTA_DIR_PATH),
            tool_loop: tree.path_inode(DEMO_THREAD_TOOL_LOOP_PATH),
            tool_loop_control: tree.path_inode(DEMO_THREAD_TOOL_LOOP_CONTROL_PATH),
            tool_loop_limits: tree.path_inode(DEMO_THREAD_TOOL_LOOP_LIMITS_PATH),
            memory_working: tree.path_inode(&["home", "1000", "memory", "working"]),
            memory_episodic: tree.path_inode(&["home", "1000", "memory", "episodic"]),
            memory_search: tree.path_inode(MEMORY_SEARCH_DIR_PATH),
            memory_semantic: tree.path_inode(MEMORY_SEMANTIC_DIR_PATH),
            memory_procedural: tree.path_inode(&["home", "1000", "memory", "procedural"]),
            memory_profile: tree.path_inode(&["home", "1000", "memory", "profile"]),
            memory_index: tree.path_inode(&["memory", "index"]),
            mcp_local_fs_server: mcp.local_fs_server,
            mcp_local_fs_control: mcp.local_fs_control,
            mcp_workspace: mcp.workspace,
            mcp_session: mcp.session,
            installed_skill_cortexfs_test: tree.path_inode(&[
                "skill",
                "installed",
                "cortexfs-test",
            ]),
            agent_helper_outbox: tree.path_inode(AGENT_HELPER_OUTBOX_PATH),
            agent_helper_control: tree.path_inode(AGENT_HELPER_CONTROL_PATH),
            agent_helper_runtime: tree.path_inode(&["agent", "helper", "runtime"]),
            collab_blackboard: tree.path_inode(&["shared", "project-a", "collab", "blackboard"]),
            collab_task_demo: tree.path_inode(&["shared", "project-a", "collab", "task", "demo"]),
            collab_handoff_demo: tree.path_inode(&[
                "shared",
                "project-a",
                "collab",
                "handoff",
                "demo",
            ]),
            collab_lock_demo: tree.path_inode(&["shared", "project-a", "collab", "lock", "demo"]),
            collab_locks: tree.path_inode(&["shared", "project-a", "collab", "lock"]),
            mcp_prompt_render_outbox: tree.path_inode(MCP_SUMMARIZE_PROMPT_RENDER_OUTBOX_PATH),
            user_policy: tree.path_inode(USER_POLICY_DIR_PATH),
            user_routes: tree.path_inode(USER_ROUTES_DIR_PATH),
            user_control: tree.path_inode(USER_CONTROL_DIR_PATH),
            user_models: tree.path_inode(USER_MODELS_DIR_PATH),
            user_models_by_provider: user_model_parents(tree),
            cluster_local: tree.path_inode(&["cluster", "local"]),
            cluster_worker: tree.path_inode(&["cluster", "local", "worker", "local-worker"]),
            cluster_control: tree.path_inode(CLUSTER_LOCAL_CONTROL_PATH),
            cluster_pending: tree.path_inode(CLUSTER_TASK_PENDING_PATH),
            cluster_running: tree.path_inode(&["cluster", "local", "queue", "default", "running"]),
            cluster_tasks: tree.path_inode(CLUSTER_TASKS_PATH),
            cluster_done: tree.path_inode(CLUSTER_TASK_DONE_PATH),
            cluster_failed: tree.path_inode(CLUSTER_TASK_FAILED_PATH),
            vector_stores: vector_store_parents(tree),
            pgvector_store: tree.path_inode(&["vector", "store", "pgvector"]),
            sqlite: tree.path_inode(&["db", "sqlite"]),
            postgres: tree.path_inode(&["db", "postgres"]),
            postgres_dsn: tree.path_inode(POSTGRES_DSN_DIR_PATH),
            provider_parents: provider_runtime_parents(tree),
        }
    }
}

fn vector_store_parents(tree: &StaticTree) -> BTreeMap<&'static str, Inode> {
    ["local", "pgvector", "qdrant", "milvus"]
        .into_iter()
        .filter_map(|store| {
            tree.path_inode_owned(&["vector", "store", store].map(String::from))
                .map(|inode| (store, inode))
        })
        .collect()
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

fn provider_runtime_parents(tree: &StaticTree) -> BTreeMap<&'static str, ProviderRuntimeParents> {
    PROVIDER_SPECS
        .iter()
        .filter_map(|provider| {
            let url = tree.path_inode_owned(&provider_child_path(provider.id, "url"))?;
            let enabled = tree.path_inode_owned(&provider_child_path(provider.id, "enabled"))?;
            let health = tree.path_inode_owned(&provider_child_path(provider.id, "health"))?;
            let models = tree.path_inode_owned(&provider_child_path(provider.id, "model"))?;
            let secrets = tree.path_inode_owned(&provider_child_path(provider.id, "secrets"))?;
            Some((
                provider.id,
                ProviderRuntimeParents {
                    url,
                    enabled,
                    health,
                    models,
                    secrets,
                },
            ))
        })
        .collect()
}

fn mcp_runtime_parents(tree: &StaticTree) -> McpRuntimeParents {
    McpRuntimeParents {
        local_fs_server: tree.path_inode(&["mcp", "server", "local-fs"]),
        local_fs_control: tree.path_inode(&["mcp", "server", "local-fs", "control"]),
        workspace: tree.path_inode(&["mcp", "resource", "local-fs", "workspace"]),
        session: tree.path_inode(&["mcp", "session", "local-fs.demo"]),
    }
}
