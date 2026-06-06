use std::collections::{BTreeMap, HashMap};

use cortex_providers::InMemoryProvider;
use cortex_store::{InMemoryStore, RequestId};
use cortexd::ExecutionPlane;
use fuse3::Inode;

use crate::execution::default_execution_plane;
use crate::runtime_parents::RuntimeParents;
use crate::runtime_types::{
    AgentTask, ApiRouteInodes, ClusterTask, ConversationExportRow, MemoryItem, PendingResponse,
    PreferencePair, PromptRender, ProviderConfigInodes, UserModelAccessInodes,
};
use crate::{DYNAMIC_INODE_BASE, Node};

#[derive(Debug, Clone, Copy)]
struct ClusterRuntimeInodes {
    state: Option<Inode>,
    worker_state: Option<Inode>,
    worker_heartbeat: Option<Inode>,
    worker_load: Option<Inode>,
    worker_current_task: Option<Inode>,
    tasks_parent: Option<Inode>,
    done_parent: Option<Inode>,
}

#[derive(Debug, Clone, Copy)]
struct AgentRuntimeInodes {
    state: Option<Inode>,
    pid: Option<Inode>,
    heartbeat: Option<Inode>,
    current_thread: Option<Inode>,
    current_task: Option<Inode>,
}

#[derive(Debug, Clone, Copy)]
struct McpRuntimeInodes {
    status: Option<Inode>,
    pid: Option<Inode>,
    workspace_content: Option<Inode>,
    workspace_refresh: Option<Inode>,
    session_state: Option<Inode>,
    session_transcript: Option<Inode>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MemoryLayerInodes {
    pub working: Option<Inode>,
    pub episodic: Option<Inode>,
    pub semantic: Option<Inode>,
    pub procedural: Option<Inode>,
    pub profile: Option<Inode>,
}

#[derive(Debug, Default)]
pub struct RuntimeState {
    pub(crate) nodes: BTreeMap<Inode, Node>,
    pub(crate) parent_children: BTreeMap<Inode, Vec<Inode>>,
    pub(crate) next_inode: Inode,
    pub(crate) staged: BTreeMap<(Inode, String), Inode>,
    pub(crate) outbox: BTreeMap<(Inode, String), Inode>,
    pub(crate) pending: HashMap<RequestId, PendingResponse>,
    pub(crate) cluster_tasks: HashMap<RequestId, ClusterTask>,
    pub(crate) agent_tasks: HashMap<RequestId, AgentTask>,
    pub(crate) memory_items: HashMap<RequestId, MemoryItem>,
    pub(crate) preference_pairs: HashMap<RequestId, PreferencePair>,
    pub(crate) prompt_renders: HashMap<RequestId, PromptRender>,
    pub(crate) audit_inode: Inode,
    pub(crate) audit_usage_inode: Inode,
    pub(crate) audit_cost_inode: Inode,
    pub(crate) audit_total_events: usize,
    pub(crate) audit_staged_events: usize,
    pub(crate) audit_queued_events: usize,
    pub(crate) audit_drained_events: usize,
    pub(crate) audit_error_events: usize,
    pub(crate) audit_denied_events: usize,
    pub(crate) audit_billable_events: usize,
    pub(crate) audit_tool_calls: usize,
    pub(crate) audit_agent_tasks: usize,
    pub(crate) conversation_rows: Vec<ConversationExportRow>,
    pub(crate) conversations_export_inode: Option<Inode>,
    pub(crate) sft_export_inode: Option<Inode>,
    pub(crate) preference_export_inode: Option<Inode>,
    pub(crate) tool_calls_export_inode: Option<Inode>,
    pub(crate) agent_traces_export_inode: Option<Inode>,
    pub(crate) export_refresh_inode: Option<Inode>,
    pub(crate) export_filter_provider_inode: Option<Inode>,
    pub(crate) export_filter_model_inode: Option<Inode>,
    pub(crate) export_filter_exclude_failed_inode: Option<Inode>,
    pub(crate) drain_inode: Inode,
    pub(crate) reload_inode: Inode,
    pub(crate) flush_inode: Inode,
    pub(crate) gc_inode: Inode,
    pub(crate) last_control_inode: Inode,
    pub(crate) queue_depth_inode: Inode,
    pub(crate) last_drained_inode: Inode,
    pub(crate) batch_count_inode: Option<Inode>,
    pub(crate) batch_state_inode: Option<Inode>,
    pub(crate) thread_messages_inode: Option<Inode>,
    pub(crate) thread_latest_inode: Option<Inode>,
    pub(crate) thread_state_inode: Option<Inode>,
    pub(crate) thread_fingerprint_inode: Option<Inode>,
    pub(crate) thread_continue_inode: Option<Inode>,
    pub(crate) thread_pause_inode: Option<Inode>,
    pub(crate) thread_cancel_inode: Option<Inode>,
    pub(crate) external_thread_messages_inode: Option<Inode>,
    pub(crate) external_thread_latest_inode: Option<Inode>,
    pub(crate) external_thread_state_inode: Option<Inode>,
    pub(crate) external_thread_fingerprint_inode: Option<Inode>,
    pub(crate) external_subject_quota_requests_inode: Option<Inode>,
    pub(crate) tool_loop_state_inode: Option<Inode>,
    pub(crate) tool_loop_steps_inode: Option<Inode>,
    pub(crate) tool_loop_continue_inode: Option<Inode>,
    pub(crate) tool_loop_pause_inode: Option<Inode>,
    pub(crate) tool_loop_cancel_inode: Option<Inode>,
    pub(crate) tool_loop_max_steps_inode: Option<Inode>,
    pub(crate) tool_loop_max_time_ms_inode: Option<Inode>,
    pub(crate) tool_loop_max_cost_usd_inode: Option<Inode>,
    pub(crate) memory_query_inode: Option<Inode>,
    pub(crate) memory_results_inode: Option<Inode>,
    pub(crate) memory_semantic_items_inode: Option<Inode>,
    pub(crate) memory_layer_items: MemoryLayerInodes,
    pub(crate) mcp_local_fs_status_inode: Option<Inode>,
    pub(crate) mcp_local_fs_pid_inode: Option<Inode>,
    pub(crate) mcp_local_fs_start_inode: Option<Inode>,
    pub(crate) mcp_local_fs_stop_inode: Option<Inode>,
    pub(crate) mcp_local_fs_restart_inode: Option<Inode>,
    pub(crate) mcp_local_fs_reload_inode: Option<Inode>,
    pub(crate) mcp_workspace_content_inode: Option<Inode>,
    pub(crate) mcp_workspace_refresh_inode: Option<Inode>,
    pub(crate) mcp_session_state_inode: Option<Inode>,
    pub(crate) mcp_session_transcript_inode: Option<Inode>,
    pub(crate) agent_helper_outbox_parent: Option<Inode>,
    pub(crate) agent_helper_start_inode: Option<Inode>,
    pub(crate) agent_helper_stop_inode: Option<Inode>,
    pub(crate) agent_helper_restart_inode: Option<Inode>,
    pub(crate) agent_helper_pause_inode: Option<Inode>,
    pub(crate) agent_helper_runtime_state_inode: Option<Inode>,
    pub(crate) agent_helper_runtime_pid_inode: Option<Inode>,
    pub(crate) agent_helper_runtime_heartbeat_inode: Option<Inode>,
    pub(crate) agent_helper_runtime_current_thread_inode: Option<Inode>,
    pub(crate) agent_helper_runtime_current_task_inode: Option<Inode>,
    pub(crate) collab_task_owner_inode: Option<Inode>,
    pub(crate) collab_task_state_inode: Option<Inode>,
    pub(crate) collab_task_events_inode: Option<Inode>,
    pub(crate) collab_locks_parent: Option<Inode>,
    pub(crate) user_allowed_providers_inode: Option<Inode>,
    pub(crate) user_default_provider_inode: Option<Inode>,
    pub(crate) user_reload_inode: Option<Inode>,
    pub(crate) user_gc_inode: Option<Inode>,
    pub(crate) user_models_refresh_inode: Option<Inode>,
    pub(crate) user_models_count_inode: Option<Inode>,
    pub(crate) user_models_list_inode: Option<Inode>,
    pub(crate) user_routes: BTreeMap<&'static str, ApiRouteInodes>,
    pub(crate) user_model_access: BTreeMap<&'static str, UserModelAccessInodes>,
    pub(crate) cluster_state_inode: Option<Inode>,
    pub(crate) cluster_worker_state_inode: Option<Inode>,
    pub(crate) cluster_worker_heartbeat_inode: Option<Inode>,
    pub(crate) cluster_worker_load_inode: Option<Inode>,
    pub(crate) cluster_worker_current_task_inode: Option<Inode>,
    pub(crate) cluster_rebalance_inode: Option<Inode>,
    pub(crate) cluster_drain_inode: Option<Inode>,
    pub(crate) cluster_pause_inode: Option<Inode>,
    pub(crate) feedback_preference_outbox_parent: Option<Inode>,
    pub(crate) mcp_prompt_render_outbox_parent: Option<Inode>,
    pub(crate) cluster_tasks_parent: Option<Inode>,
    pub(crate) cluster_done_parent: Option<Inode>,
    pub(crate) pgvector_enabled_inode: Option<Inode>,
    pub(crate) pgvector_status_inode: Option<Inode>,
    pub(crate) pgvector_collections_inode: Option<Inode>,
    pub(crate) pgvector_refresh_inode: Option<Inode>,
    pub(crate) postgres_status_inode: Option<Inode>,
    pub(crate) postgres_dsn_current_inode: Option<Inode>,
    pub(crate) postgres_dsn_effective_inode: Option<Inode>,
    pub(crate) postgres_dsn_source_inode: Option<Inode>,
    pub(crate) provider_base_url: BTreeMap<&'static str, ProviderConfigInodes>,
    pub(crate) provider_enabled: BTreeMap<&'static str, ProviderConfigInodes>,
    pub(crate) provider_health_check: BTreeMap<&'static str, Inode>,
    pub(crate) provider_health_status: BTreeMap<&'static str, Inode>,
    pub(crate) provider_health_latency_ms: BTreeMap<&'static str, Inode>,
    pub(crate) provider_health_last_error: BTreeMap<&'static str, Inode>,
    pub(crate) provider_secret_rotate: BTreeMap<&'static str, Inode>,
    pub(crate) provider_secret_last_rotated: BTreeMap<&'static str, Inode>,
    pub(crate) provider_secret_next_rotation: BTreeMap<&'static str, Inode>,
    pub(crate) provider_models_refresh: BTreeMap<&'static str, Inode>,
    pub(crate) batch_count: usize,
    pub(crate) plane: Option<ExecutionPlane<InMemoryStore, InMemoryProvider>>,
}

impl RuntimeState {
    pub(crate) fn new(parents: &RuntimeParents) -> Self {
        let mut state = Self::blank(parents);
        state.attach_runtime_files(parents);
        state.refresh_provider_health_statuses();
        state.refresh_user_model_access();
        state.refresh_user_routes();
        state
    }

    fn blank(parents: &RuntimeParents) -> Self {
        let mcp = mcp_runtime_inodes(parents);
        let agent = agent_runtime_inodes(parents);
        let cluster = cluster_runtime_inodes(parents);
        Self {
            next_inode: DYNAMIC_INODE_BASE,
            audit_inode: DYNAMIC_INODE_BASE,
            audit_usage_inode: DYNAMIC_INODE_BASE,
            audit_cost_inode: DYNAMIC_INODE_BASE,
            drain_inode: DYNAMIC_INODE_BASE,
            reload_inode: DYNAMIC_INODE_BASE,
            flush_inode: DYNAMIC_INODE_BASE,
            gc_inode: DYNAMIC_INODE_BASE,
            last_control_inode: DYNAMIC_INODE_BASE,
            queue_depth_inode: DYNAMIC_INODE_BASE,
            last_drained_inode: DYNAMIC_INODE_BASE,
            external_subject_quota_requests_inode: parents.external_subject_quota_requests,
            mcp_local_fs_status_inode: mcp.status,
            mcp_local_fs_pid_inode: mcp.pid,
            mcp_workspace_content_inode: mcp.workspace_content,
            mcp_workspace_refresh_inode: mcp.workspace_refresh,
            mcp_session_state_inode: mcp.session_state,
            mcp_session_transcript_inode: mcp.session_transcript,
            agent_helper_outbox_parent: parents.agent_helper_outbox,
            agent_helper_runtime_state_inode: agent.state,
            agent_helper_runtime_pid_inode: agent.pid,
            agent_helper_runtime_heartbeat_inode: agent.heartbeat,
            agent_helper_runtime_current_thread_inode: agent.current_thread,
            agent_helper_runtime_current_task_inode: agent.current_task,
            collab_task_owner_inode: parents.collab_task_owner,
            collab_task_state_inode: parents.collab_task_state,
            collab_task_events_inode: parents.collab_task_events,
            collab_locks_parent: parents.collab_locks,
            cluster_state_inode: cluster.state,
            cluster_worker_state_inode: cluster.worker_state,
            cluster_worker_heartbeat_inode: cluster.worker_heartbeat,
            cluster_worker_load_inode: cluster.worker_load,
            cluster_worker_current_task_inode: cluster.worker_current_task,
            feedback_preference_outbox_parent: parents.feedback_preference_outbox,
            mcp_prompt_render_outbox_parent: parents.mcp_prompt_render_outbox,
            cluster_tasks_parent: cluster.tasks_parent,
            cluster_done_parent: cluster.done_parent,
            pgvector_enabled_inode: parents.pgvector_enabled,
            pgvector_status_inode: parents.pgvector_status,
            pgvector_collections_inode: parents.pgvector_collections,
            pgvector_refresh_inode: parents.pgvector_refresh,
            postgres_status_inode: parents.postgres_status,
            plane: default_execution_plane(),
            ..Self::default()
        }
    }
}

const fn mcp_runtime_inodes(parents: &RuntimeParents) -> McpRuntimeInodes {
    McpRuntimeInodes {
        status: parents.mcp_local_fs_status,
        pid: parents.mcp_local_fs_pid,
        workspace_content: parents.mcp_workspace_content,
        workspace_refresh: parents.mcp_workspace_refresh,
        session_state: parents.mcp_session_state,
        session_transcript: parents.mcp_session_transcript,
    }
}

const fn agent_runtime_inodes(parents: &RuntimeParents) -> AgentRuntimeInodes {
    AgentRuntimeInodes {
        state: parents.agent_helper_runtime_state,
        pid: parents.agent_helper_runtime_pid,
        heartbeat: parents.agent_helper_runtime_heartbeat,
        current_thread: parents.agent_helper_runtime_current_thread,
        current_task: parents.agent_helper_runtime_current_task,
    }
}

const fn cluster_runtime_inodes(parents: &RuntimeParents) -> ClusterRuntimeInodes {
    ClusterRuntimeInodes {
        state: parents.cluster_state,
        worker_state: parents.cluster_worker_state,
        worker_heartbeat: parents.cluster_worker_heartbeat,
        worker_load: parents.cluster_worker_load,
        worker_current_task: parents.cluster_worker_current_task,
        tasks_parent: parents.cluster_tasks,
        done_parent: parents.cluster_done,
    }
}
