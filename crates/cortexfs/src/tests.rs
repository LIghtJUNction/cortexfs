use super::agent::create::{
    AgentCreateError, AgentCreateStage, AgentRollbackError, AgentRollbackStage, chown_home_entry,
    create_agent_files, create_agent_files_with_hook, rollback_agent_files,
    rollback_agent_files_with_hook,
};
use super::runtime::record::socket::record_unindexed_socket_request_for_test;
use super::support::columnar;
use super::support::index::set_session_index_update_failure;
use super::{
    AGENT_CONTROL_FILES, ATIF_SCHEMA_VERSION, AgentControlKind, AgentExecutableRunRequest,
    AgentExecutableSocketRuntime, AgentPermissions, AgentRuntimeViewError, AgentScheduleAdvance,
    AgentScheduleChildHandoff, AgentScheduleIssue, AgentScheduleNodeKind, AgentScheduleRecordError,
    AgentUnixIdentity, AgentWindowSetting, ApiKeyResolutionError, BOOTSTRAP_STATE_REL,
    BootstrapAction, BootstrapState, BwrapAgentExecutableArgs, CHILD_RESULT_REQUIRED_FILES,
    CONTEXT_REQUIRED_FILES, CORTEXFS_OBJECT_RUNNER, CTX_ROOT, ChildAgentAuthority,
    ChildAgentControls, ChildAgentDenial, ChildAgentRequest, ChildClaimStage,
    ChildContextRecordError, ChildContextStatus, ChildHandoffStage, ChildLifecycle,
    ContextJsonlIssue, ContextJsonlKind, ControlLineIssue, DEFAULT_WORKER_MODEL,
    DurableSessionLayoutError, EXEC_OBJECTS, EventStreamIssue, FUSE_ROOT_INODE, FuseDirEntry,
    FuseError, FuseFileType, FuseProjection, IndexedSocketSessionRecordError, LayoutPathRole,
    MAX_AGENT_SCHEDULE_NODES, MAX_ECHO_MODEL_STDIN_BYTES, MAX_FUSE_SMALL_WRITE_BYTES,
    MAX_OBJECT_NAME_LEN, MAX_SOCKET_FRAME_BYTES, MIGRATION_RETIRED_AGENTS, MIGRATION_ROLLING_TREE,
    MODEL_CONTROL_FILES, MessageStreamIssue, ModelCapabilityIssue, ModelContextLimit,
    ModelDriverRouteError, ModelDriverUseCase, MountEntry, MountError, MountMode, MountOption,
    MountTable, OAuthError, OAuthPkce, OAuthProviderConfig, OBJECT_HOOK_DIR,
    OBJECT_HOOK_PHASE_DIRS, ObjectBootstrapError, ObjectClass, OwnedChildCancellationError,
    PathLayoutIssue, PeerCredentials, PolicyError, PolicyObjectClass, PolicyPermission, PolicyRule,
    PolicyV0, REFERENCE_TREE_VERSION, ReferenceTreeError, RunEnvironment, SECRET_TOOL_PROGRAM,
    SESSION_REQUIRED_FILES, SHARED_QUEUE_REQUIRED_DIRS, SessionAccess, SessionAccessAuthority,
    SessionAccessDenial, SessionControlKind, SessionIndexGuard, SessionIndexKind,
    SessionIndexUpdateError, SharedAccess, SharedAccessAuthority, SharedAccessDenial,
    SkillMetadata, SocketPeerPolicy, SocketRequest, SocketRequestError, SocketRuntimeError,
    SocketSendOutcome, SocketSessionRecordError, SocketSessionScope, TOOL_CONTROL_FILES,
    ToolExecutionAuthority, ToolExecutionDenial, ToolExecutionPrincipal, ToolHit, ToolPath,
    ToolPathError, ToolSchemaIssue, TrajectoryIssue, TrajectoryMapError, TrajectoryObservation,
    TrajectoryObservationResult, advance_agent_schedule_from_parent_context,
    agent_executable_socket_bwrap_args, agent_executable_socket_command, append_jsonl_line,
    apply_reference_tree_upgrade, atomic_create_text_with_mode,
    atomic_replace_text_preserving_metadata, atomic_replace_text_preserving_metadata_with_hook,
    atomic_replace_text_with_mode, authorize_child_agent, authorize_session_access,
    authorize_shared_access, authorize_tool_execution, chown_reference_home_entry,
    claim_child_handoff_active, claim_child_handoff_active_with_hook, classify_abi_path,
    collect_agent_rules_from_paths, collect_history_messages_from_session,
    compare_and_update_session_index, completed_agent_schedule_nodes_from_parent_context,
    create_private_context_dir, default_agent_model_for_name, derive_agent_runtime_view,
    ensure_durable_session_layout, ensure_reference_tree, format_history_messages_jsonl,
    format_skill_metadata_with_budget, fuse_metadata_error, handle_socket_request_frame,
    inspect_agent_control, inspect_agent_schedule_json, inspect_context_jsonl,
    inspect_context_pack_json, inspect_event_stream_jsonl, inspect_message_stream_jsonl,
    inspect_model_capabilities, inspect_object_layout, inspect_session_control,
    inspect_session_index, inspect_session_layout, inspect_shared_queue_layout,
    inspect_tool_schema_json, install_executable_object_wrapper, is_dedicated_worker_agent_name,
    is_object_name, is_root_entry, is_worker_agent_name, model_exec_metadata,
    oauth_authorization_code_form, oauth_authorization_url, oauth_refresh_token_form,
    open_agent_executable_no_follow, owned_child_cancellation_events, parse_model_driver_routes,
    parse_oauth_token_response, parse_socket_request_frame, peer_credentials,
    plan_reference_tree_upgrade, publish_child_handoff, publish_child_handoff_with_hook,
    read_bootstrap_state, read_echo_model_stdin_limited, ready_agent_schedule_child_handoffs,
    ready_agent_schedule_nodes, record_agent_schedule_to_parent_context,
    record_assistant_response_to_session, record_child_handoff_to_parent_context,
    record_child_result_to_parent_context, record_indexed_socket_send_to_session,
    record_owned_child_cancellation, record_ready_agent_schedule_child_handoffs_to_parent_context,
    record_tool_execution_denial_to_session, record_tool_execution_result_to_session,
    reference_agent_children, reference_agent_model, reference_agent_policy,
    reference_agent_system_prompt, resolve_api_key_from_env_names_with, resolve_api_key_with,
    resolve_fuse_abi_path, resolve_oauth_access_token_with, rollback_child_handoff, run_echo_model,
    run_secret_tool_command_with_timeout, secret_tool_dbus_address,
    serve_agent_executable_socket_stream_once, serve_unix_socket_listener_once,
    serve_unix_socket_stream_once, session_index_key_for_cwd, set_private_dir_permissions,
    set_text_file_permissions, should_repair_reference_owner, snapshot_dirs,
    socket_runtime_error_response, trajectory_from_session_dir, trajectory_from_session_jsonl,
    update_session_index, update_session_index_with_keys, validate_context_pack_source,
    validate_trajectory, write_run_snapshot, write_snapshot, write_text_file_if_absent,
    write_trajectory_json,
};
use crate::fuse::projection::core::fuse_readlink_error;
use std::fs;
use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt, symlink};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
#[macro_use]
mod helpers;

mod atomic;
mod authority;
mod channel;
mod child;
mod context;
mod defaults;
mod execution;
mod layout;
mod projection;
mod prompt;
mod provider;
mod recording;
mod reference;
mod runtime;
mod schedule;
mod session;
mod shared;
mod socket;
mod store;
mod trajectory;

use helpers::*;

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
