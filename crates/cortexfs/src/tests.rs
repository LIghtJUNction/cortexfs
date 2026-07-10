use super::{
    AGENT_CONTROL_FILES, ATIF_SCHEMA_VERSION, AgentControlKind, AgentExecutableSocketExecution,
    AgentExecutableSocketRuntime, AgentRuntimeViewError, AgentScheduleAdvance,
    AgentScheduleChildHandoff, AgentScheduleIssue, AgentScheduleNodeKind, AgentScheduleRecordError,
    AgentUnixIdentity, ApiKeyResolutionError, BOOTSTRAP_STATE_REL, BootstrapAction,
    BwrapAgentExecutableArgs, CORTEXFS_OBJECT_RUNNER, CTX_ROOT, ChildAgentAuthority,
    ChildAgentControls, ChildAgentDenial, ChildAgentRequest, ChildContextRecordError,
    ChildContextStatus, ChildLifecycle, ContextJsonlIssue, ContextJsonlKind, ContextPackBuildError,
    ContextPackIssue, ContextPackSourceError, ControlLineIssue, DEFAULT_WORKER_MODEL,
    DurableSessionLayoutError, EXEC_OBJECTS, EventStreamIssue, FUSE_V1_ROOT_INODE, FuseV1Error,
    FuseV1FileType, FuseV1Projection, IndexedSocketSessionRecordError, LayoutPathRole,
    MAX_AGENT_SCHEDULE_NODES, MAX_ECHO_MODEL_STDIN_BYTES, MAX_FUSE_V1_SMALL_WRITE_BYTES,
    MAX_OBJECT_NAME_LEN, MAX_SOCKET_FRAME_BYTES, MIGRATION_RETIRED_AGENTS_V1, MODEL_CONTROL_FILES,
    MessageStreamIssue, ModelCapabilityIssue, ModelDriverRouteError, ModelDriverUseCase,
    MountEntry, MountError, MountMode, MountOption, MountTable, OAuthError, OAuthPkce,
    OAuthProviderConfig, OBJECT_HOOK_DIR, OBJECT_HOOK_PHASE_DIRS, ObjectBootstrapError,
    ObjectClass, OwnedChildCancellationError, PathLayoutIssue, PeerCredentials, PolicyError,
    PolicyObjectClass, PolicyPermission, PolicyRule, PolicyV0, REFERENCE_TREE_VERSION,
    ReferenceTreeError, SESSION_REQUIRED_FILES, SHARED_QUEUE_REQUIRED_DIRS, SessionAccess,
    SessionAccessAuthority, SessionAccessDenial, SessionControlKind, SessionIndexKind,
    SessionIndexUpdateError, SharedAccess, SharedAccessAuthority, SharedAccessDenial,
    SharedQueueClaimError, SharedQueueFinishError, SharedQueueOutcome, SharedQueueRecoverError,
    SkillMetadata, SocketPeerPolicy, SocketRequest, SocketRequestError, SocketRuntimeError,
    SocketSessionRecordError, SocketSessionScope, TOOL_CONTROL_FILES, ToolExecutionAuthority,
    ToolExecutionDenial, ToolExecutionPrincipal, ToolHit, ToolPath, ToolPathError, ToolSchemaIssue,
    TrajectoryIssue, TrajectoryMapError, TrajectoryObservation, TrajectoryObservationResult,
    advance_agent_schedule_from_parent_context, agent_executable_socket_bwrap_args,
    append_jsonl_line, apply_reference_tree_upgrade, atomic_create_text_with_mode,
    atomic_replace_text_preserving_metadata, atomic_replace_text_preserving_metadata_with_hook,
    atomic_replace_text_with_mode, authorize_child_agent, authorize_session_access,
    authorize_shared_access, authorize_tool_execution, claim_next_shared_queue_job,
    classify_abi_path, collect_agent_rules_from_paths, collect_history_messages_from_session,
    completed_agent_schedule_nodes_from_parent_context, create_private_context_dir,
    default_agent_model_for_name, derive_agent_runtime_view, ensure_durable_session_layout,
    ensure_v1_reference_tree, finish_shared_queue_job, format_history_messages_jsonl,
    format_skill_metadata_with_budget, fuse_metadata_error, handle_socket_request_frame,
    inspect_agent_control, inspect_agent_schedule_json, inspect_context_jsonl,
    inspect_context_pack_json, inspect_event_stream_jsonl, inspect_message_stream_jsonl,
    inspect_model_capabilities, inspect_object_layout, inspect_session_control,
    inspect_session_index, inspect_session_layout, inspect_shared_queue_layout,
    inspect_tool_schema_json, install_executable_object_wrapper, is_dedicated_worker_agent_name,
    is_object_name, is_root_entry, is_worker_agent_name, model_exec_metadata,
    oauth_authorization_code_form, oauth_authorization_url, oauth_refresh_token_form,
    owned_child_cancellation_events, parse_model_driver_routes, parse_oauth_token_response,
    parse_socket_request_frame, peer_credentials, plain_fs, plan_reference_tree_upgrade,
    read_bootstrap_state, read_echo_model_stdin_limited, ready_agent_schedule_child_handoffs,
    ready_agent_schedule_nodes, rebuild_context_pack, record_agent_schedule_to_parent_context,
    record_assistant_response_to_session, record_child_handoff_to_parent_context,
    record_child_result_to_parent_context, record_indexed_socket_send_to_session,
    record_owned_child_cancellation, record_ready_agent_schedule_child_handoffs_to_parent_context,
    record_socket_request_to_session, record_tool_execution_denial_to_session,
    record_tool_execution_result_to_session, recover_shared_queue_job,
    reference_agent_system_prompt, resolve_api_key_from_env_names_with, resolve_api_key_with,
    resolve_fuse_abi_path, resolve_oauth_access_token_with, run_echo_model,
    serve_agent_executable_socket_stream_once, serve_unix_socket_listener_once,
    serve_unix_socket_stream_once, session_index_key_for_cwd, set_private_dir_permissions,
    set_text_file_permissions, should_repair_reference_owner, snapshot_dirs,
    socket_runtime_error_response, trajectory_from_session_dir, trajectory_from_session_jsonl,
    update_session_index, update_session_index_with_keys, validate_context_pack_source,
    validate_trajectory, write_run_snapshot, write_snapshot, write_text_file_if_absent,
    write_trajectory_json,
};
use crate::fuse::v1_projection::core::fuse_readlink_error;
use std::fs;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt, symlink};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
include!("../tests/unit/lib/helpers.rs");
include!("../tests/unit/lib/agent_model_defaults.rs");
include!("../tests/unit/lib/reference_tree_basics.rs");
include!("../tests/unit/lib/agent_prompt.rs");
include!("../tests/unit/lib/fuse_projection_objects.rs");
include!("../tests/unit/lib/object_agent_layout.rs");
include!("../tests/unit/lib/agent_runtime_socket_parse.rs");
include!("../tests/unit/lib/socket_session_record.rs");
include!("../tests/unit/lib/socket_runtime.rs");
include!("../tests/unit/lib/agent_execution_policy.rs");
include!("../tests/unit/lib/agent_schedule.rs");
include!("../tests/unit/lib/child_context.rs");
include!("../tests/unit/lib/session_context.rs");
include!("../tests/unit/lib/context_queue.rs");
include!("../tests/unit/lib/shared_queue_access.rs");
include!("../tests/unit/lib/tool_authority.rs");
include!("../tests/unit/lib/trajectory/map_and_validate.rs");
include!("../tests/unit/lib/atomic_replace.rs");

/// Locks the mechanical src-tree naming rule from `docs/naming-guide.md`.
///
/// New modules use single-token stems without `-` or `_`; legacy multi-word
/// stems remain until intentionally migrated, so this test only enforces the
/// repository-wide `mod.rs` ban.
#[test]
fn src_tree_has_no_mod_rs() {
    let src_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    assert!(
        src_root.is_dir(),
        "expected crate src directory at {}",
        src_root.display()
    );

    let mut mod_rs_files = Vec::new();
    let mut stack = vec![src_root];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                stack.push(path);
                continue;
            }
            // Skip non-regular entries (sockets, fifos, etc.).
            if file_type.is_symlink() {
                continue;
            }
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            if name == "mod.rs" {
                mod_rs_files.push(path);
            }
        }
    }

    assert!(
        mod_rs_files.is_empty(),
        "mod.rs is forbidden under crates/cortexfs/src; found: {mod_rs_files:?}"
    );
}

/// Locks shared quality models: domain issue aliases share the control-line and
/// path-layout bases (no parallel `EmptyValue` / `MissingFile` enums).
#[test]
fn shared_issue_aliases_use_control_line_and_path_layout_bases() {
    use super::{
        AgentControlIssue, ObjectLayoutIssue, SessionControlIssue, SessionIndexIssue,
        SessionLayoutIssue, SharedQueueLayoutIssue, ToolSchemaIssue,
    };
    use std::any::TypeId;

    assert_eq!(
        TypeId::of::<AgentControlIssue>(),
        TypeId::of::<ControlLineIssue>()
    );
    assert_eq!(
        TypeId::of::<SessionControlIssue>(),
        TypeId::of::<ControlLineIssue>()
    );
    assert_eq!(
        TypeId::of::<SessionIndexIssue>(),
        TypeId::of::<ControlLineIssue>()
    );
    assert_eq!(
        TypeId::of::<ToolSchemaIssue>(),
        TypeId::of::<ControlLineIssue>()
    );
    assert_eq!(
        TypeId::of::<ObjectLayoutIssue>(),
        TypeId::of::<PathLayoutIssue>()
    );
    assert_eq!(
        TypeId::of::<SessionLayoutIssue>(),
        TypeId::of::<PathLayoutIssue>()
    );
    assert_eq!(
        TypeId::of::<SharedQueueLayoutIssue>(),
        TypeId::of::<PathLayoutIssue>()
    );

    // Drive shipped helpers so the bases are not dead aliases.
    let line_issues = super::inspect_agent_control(super::AgentControlKind::Uid, "not-a-number");
    assert!(
        line_issues
            .issues()
            .iter()
            .any(|issue| matches!(issue, ControlLineIssue::InvalidNumber { .. }))
    );

    let layout = PathLayoutIssue::missing("tool/x", LayoutPathRole::Executable);
    assert_eq!(layout.kind(), "missing executable");
    assert_eq!(layout.path(), "tool/x");
}
