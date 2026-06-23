use super::{
    authorize_child_agent, authorize_session_access, authorize_shared_access,
    authorize_tool_execution, claim_next_shared_queue_job, classify_abi_path,
    derive_agent_runtime_view, ensure_durable_session_layout, ensure_v1_reference_tree,
    finish_shared_queue_job, handle_socket_request_frame, inspect_agent_control,
    inspect_context_jsonl, inspect_context_pack_json, inspect_event_stream_jsonl,
    inspect_message_stream_jsonl, inspect_model_capabilities, inspect_object_layout,
    inspect_session_control, inspect_session_index, inspect_session_layout,
    inspect_shared_queue_layout, inspect_tool_schema_json, install_executable_object_wrapper,
    is_object_name, is_root_entry, model_exec_metadata, owned_child_cancellation_events,
    parse_model_driver_routes, parse_socket_request_frame, peer_credentials, rebuild_context_pack,
    record_assistant_response_to_session, record_child_handoff_to_parent_context,
    record_child_result_to_parent_context, record_indexed_socket_send_to_session,
    record_owned_child_cancellation, record_socket_request_to_session,
    record_tool_execution_denial_to_session, record_tool_execution_result_to_session,
    recover_shared_queue_job, resolve_api_key_with, run_echo_model,
    serve_agent_executable_socket_stream_once, serve_unix_socket_listener_once,
    serve_unix_socket_stream_once, session_index_key_for_cwd, socket_runtime_error_response,
    update_session_index, validate_context_pack_source,
    AgentControlIssue, AgentControlKind, AgentExecutableSocketRuntime, AgentRuntimeViewError,
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
    SharedAccess, SharedAccessAuthority, SharedAccessDenial, SharedQueueLayoutIssue,
    SharedQueueOutcome, SharedQueueRecoverError, SocketPeerPolicy, SocketRequest, SocketRequestError,
    SocketRuntimeError,
    SocketSessionRecordError, SocketSessionScope, ToolExecutionAuthority, ToolExecutionDenial,
    ToolExecutionPrincipal, ToolHit, ToolPath, ToolPathError, ToolSchemaIssue, AGENT_CONTROL_FILES,
    CORTEXFS_OBJECT_RUNNER, CTX_ROOT, EXEC_OBJECTS, FUSE_V1_ROOT_INODE,
    MAX_FUSE_V1_SMALL_WRITE_BYTES, MAX_OBJECT_NAME_LEN, MAX_SOCKET_FRAME_BYTES,
    MODEL_CONTROL_FILES, SESSION_REQUIRED_FILES, SHARED_QUEUE_REQUIRED_DIRS, TOOL_CONTROL_FILES,
};
use std::fs;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::{symlink, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
include!("lib/013_helpers.rs");
include!("lib/001_basics_reference.rs");
include!("lib/002_fuse_object.rs");
include!("lib/003_object_agent.rs");
include!("lib/004_agent_api_socket_parse.rs");
include!("lib/005_socket_record.rs");
include!("lib/006_socket_runtime.rs");
include!("lib/007_agent_exec_policy.rs");
include!("lib/008_child_context.rs");
include!("lib/009_session_context.rs");
include!("lib/010_context_queue.rs");
include!("lib/011_queue_access.rs");
include!("lib/012_tool_authority.rs");
