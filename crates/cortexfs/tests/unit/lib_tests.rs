use super::{
    append_jsonl_line, atomic_replace_text_with_mode, authorize_child_agent, authorize_session_access, authorize_shared_access,
    authorize_tool_execution, claim_next_shared_queue_job, classify_abi_path,
    collect_agent_rules_from_paths, collect_history_messages_from_session, collect_skill_metadata,
    create_private_context_dir,
    derive_agent_runtime_view, ensure_durable_session_layout, ensure_v1_reference_tree, finish_shared_queue_job,
    format_history_messages_jsonl, format_skill_metadata_with_budget, handle_socket_request_frame,
    inspect_agent_control, inspect_context_jsonl, inspect_context_pack_json, inspect_event_stream_jsonl, inspect_message_stream_jsonl,
    inspect_agent_schedule_json, ready_agent_schedule_child_handoffs, ready_agent_schedule_nodes,
    inspect_model_capabilities, inspect_object_layout, inspect_session_control,
    inspect_session_index, inspect_session_layout, inspect_shared_queue_layout,
    inspect_tool_schema_json, install_executable_object_wrapper, is_object_name, is_root_entry,
    model_exec_metadata, owned_child_cancellation_events, parse_model_driver_routes,
    parse_socket_request_frame, peer_credentials, read_echo_model_stdin_limited,
    advance_agent_schedule_from_parent_context, completed_agent_schedule_nodes_from_parent_context,
    rebuild_context_pack,
    record_assistant_response_to_session, record_child_handoff_to_parent_context,
    record_child_result_to_parent_context, record_indexed_socket_send_to_session,
    record_agent_schedule_to_parent_context, record_owned_child_cancellation,
    record_ready_agent_schedule_child_handoffs_to_parent_context, record_socket_request_to_session,
    record_tool_execution_denial_to_session, record_tool_execution_result_to_session,
    recover_shared_queue_job, resolve_api_key_from_env_names_with, resolve_api_key_with,
    resolve_fuse_abi_path, resolve_oauth_access_token_with, run_echo_model,
    serve_agent_executable_socket_stream_once, serve_unix_socket_listener_once,
    serve_unix_socket_stream_once, session_index_key_for_cwd, socket_runtime_error_response,
    sync_plain_directory,
    set_private_dir_permissions, set_text_file_permissions, update_session_index,
    update_session_index_with_keys, validate_context_pack_source, write_text_file_if_absent,
    oauth_authorization_code_form, oauth_authorization_url, oauth_refresh_token_form,
    parse_oauth_token_response,
    AgentControlIssue, AgentControlKind, AgentExecutableSocketRuntime, AgentRuntimeViewError,
    AgentScheduleAdvance, AgentScheduleChildHandoff, AgentScheduleIssue, AgentScheduleNodeKind,
    AgentScheduleRecordError,
    AgentUnixIdentity, ApiKeyResolutionError, ChildAgentAuthority, ChildAgentControls,
    ChildAgentDenial, ChildAgentRequest, ChildContextRecordError, ChildContextStatus,
    ChildLifecycle, ContextJsonlIssue, ContextJsonlKind, ContextPackBuildError,
    ContextPackIssue, ContextPackSourceError, DurableSessionLayoutError, EventStreamIssue,
    FuseV1Error, FuseV1FileType, FuseV1Projection, IndexedSocketSessionRecordError,
    MessageStreamIssue, ModelCapabilityIssue, ModelDriverRouteError, ModelDriverUseCase,
    MountEntry, MountError, MountMode, MountOption, MountTable, ObjectBootstrapError, ObjectClass,
    ObjectLayoutIssue, OwnedChildCancellationError, PeerCredentials, PolicyError,
    PolicyObjectClass, PolicyPermission, PolicyRule, PolicyV0, ReferenceTreeError, SessionAccess,
    SessionAccessAuthority, SessionAccessDenial, SessionControlIssue, SessionControlKind,
    SessionIndexIssue, SessionIndexKind, SessionIndexUpdateError, SessionLayoutIssue,
    SharedAccess, SharedAccessAuthority, SharedAccessDenial, SharedQueueClaimError,
    SharedQueueFinishError, SharedQueueLayoutIssue, SkillMetadata, SharedQueueOutcome,
    SharedQueueRecoverError, SocketPeerPolicy, SocketRequest, SocketRequestError,
    OAuthError, OAuthPkce, OAuthProviderConfig, SocketRuntimeError,
    SocketSessionRecordError, SocketSessionScope, ToolExecutionAuthority, ToolExecutionDenial,
    ToolExecutionPrincipal, ToolHit, ToolPath, ToolPathError, ToolSchemaIssue, AGENT_CONTROL_FILES,
    CORTEXFS_OBJECT_RUNNER, CTX_ROOT, EXEC_OBJECTS, FUSE_V1_ROOT_INODE,
    MAX_ECHO_MODEL_STDIN_BYTES, MAX_FUSE_V1_SMALL_WRITE_BYTES, MAX_OBJECT_NAME_LEN,
    MAX_SOCKET_FRAME_BYTES, MODEL_CONTROL_FILES, SESSION_REQUIRED_FILES,
    SHARED_QUEUE_REQUIRED_DIRS, TOOL_CONTROL_FILES,
};
use std::fs;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::{symlink, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
include!("lib/helpers.rs");
include!("lib/reference_tree_basics.rs");
include!("lib/agent_prompt.rs");
include!("lib/fuse_projection_objects.rs");
include!("lib/object_agent_layout.rs");
include!("lib/agent_runtime_socket_parse.rs");
include!("lib/socket_session_record.rs");
include!("lib/socket_runtime.rs");
include!("lib/agent_execution_policy.rs");
include!("lib/agent_schedule.rs");
include!("lib/child_context.rs");
include!("lib/session_context.rs");
include!("lib/context_queue.rs");
include!("lib/shared_queue_access.rs");
include!("lib/tool_authority.rs");
include!("lib/atomic_replace.rs");
