use super::{
    AGENT_PROFILE_SCHEMA_V1, AgentArgs, AgentChildRow, AgentLaunchCommand, AgentMount,
    AgentNewArgs, AgentProcess, AgentProfile, AgentSessionArchiveArgs, AgentSessionGcArgs,
    AgentStartArgs, AgentUnixIdentity, BOOTSTRAP_REFERENCE_AGENT_SUMMARY_LINE, Cli, CliError,
    Command, ControlLineIssue, FileCommand, LayoutPathRole, LsTarget, MAX_AGENT_EVENTS,
    MAX_AGENT_RESPONSE_BYTES, MAX_BUFFERED_AGENT_DIAGNOSTICS, MAX_BUFFERED_AGENT_EVENTS,
    MAX_BUFFERED_AGENT_RENDERED_BYTES, MAX_OAUTH_CALLBACK_REQUEST_BYTES,
    MAX_PROVIDER_SECRET_STDIN_BYTES, MAX_SOCKET_FRAME_BYTES, ObjectClass, PathLayoutIssue,
    ProviderArgs, ScheduleArgs, ScheduleChildContextAbiPaths, ScheduleResultInput, TrajectoryIssue,
    absolute_existing_path, adopt_default_source_root, agent_apply, agent_bwrap_args,
    agent_chat_request_socket,
    agent_chat_runtime_socket, agent_chat_socket_systemd_command, agent_chat_unit,
    agent_child_rows, agent_env_lines, agent_host_mount_source, agent_inspect_lines,
    agent_lifecycle_tool_command,
    agent_lifecycle_tool_selected, agent_new, agent_new_args_from_profile, agent_new_host_fallback,
    agent_new_request_json, agent_runtime_context_matches_values, agent_send_request_json,
    agent_session_archive, agent_session_gc, agent_start_mounts_with_default_source,
    agent_start_sandbox_cwd, agent_start_status_lines,
    agent_start_systemd_command, agent_status_lines, agent_stop, agent_terminal_socket,
    agent_trajectory, agent_wait, append_agent_log_event, atomic_write_provider_config,
    bootstrap_reference_tree_default, build_agent_system_prompt, cat_path, child_wait_exit_code,
    classify_input_path, cli_error_line, collect_agent_events_buffered, cortexfs_mount_bin,
    cortexfs_xattr_line,
    create_agent_terminal_runtime_dir, create_plain_mountpoint_dir, ctx_home,
    ctx_root_entry_present, ctx_root_shape, ctx_state, current_session_name,
    debug_timing_diagnostic, detached_mount_command, direct_mount_command, doctor,
    doctor_bootstrap_state, doctor_report_line, doctor_retired_reference_agents, doctor_root_line,
    doctor_unexpected_entry_line, ensure_agent_terminal_socket,
    ensure_best_effort_visible_terminal_socket, ensure_plain_mountpoint_dir, env_exports,
    file_append, file_check, file_set, file_type_name, format_agent_control_issues,
    format_agent_schedule_issues, format_context_jsonl_issues, format_context_pack_issues,
    format_event_stream_issues, format_message_stream_issues, format_model_capability_issues,
    format_model_driver_route_error, format_object_layout_issues, format_session_control_issues,
    format_session_index_issues, format_session_layout_issues, format_shared_queue_layout_issues,
    format_tool_schema_issues, format_trajectory_issue, gc_archive_agent_root, is_mount_point,
    json_string, latest_run_id, list_names, load_agent_profile, load_schedule_context,
    object_execution_command, open_executable_no_follow, parse, parse_agent_profile_text,
    parse_command, parse_oauth_callback_params, plain_sibling_mount_bin, print_help_topic,
    provider_preset, read_agent_processes, read_ctx_status, read_file_to_string,
    read_oauth_callback_request_from_reader, read_optional_trimmed, read_provider_config_file,
    read_provider_config_from_dir, read_provider_secret_stdin_limited, read_status_agent_processes,
    record_agent_start_state, remove_stale_socket, remove_temp_agent_object,
    render_agent_event_lines, render_agent_process_tree, render_agent_status_lines,
    require_agent_mount, require_cli_name, require_session_name, resolve_abi_path,
    resolve_agent_profile_path, resolve_child_wait_status, rollback_agent_late_chat_start_with,
    rollback_agent_start_resources_with, run, run_visible_tool, run_visible_tool_with_writer,
    schedule_child_context_abi_paths, schedule_claim_with_hook, schedule_command,
    schedule_context_abi_path, schedule_handoff_agent_details, schedule_parent_ref_for_output,
    schedule_require_handoff_parent, schedule_result_with_hook, schedule_status_lines,
    set_gc_delete_fault_for_test, set_gc_delete_sync_fault_for_test,
    set_gc_list_publish_replacement_for_test, set_gc_list_rollback_replacement_for_test,
    set_gc_source_claim_fault_for_test, set_profile_tools_policy_fault, shell_quote_arg,
    socket_bind_path, stream_socket_request, stream_terminal_socket, system_agent_socket_command,
    systemctl_user_command, temp_file_name,
    terminal_connect_cli_error, terminal_safe_text, terminal_socket_exists, waiting_diagnostic,
    write_agent_control_plain, write_agent_session_plain,
};
use cortexfs::agent::launch::{AgentChatAliasState, ensure_agent_chat_socket};
use cortexfs::parse_main_pid;
use cortexfs::{
    AGENT_CONTROL_FILES, AbiPathKind, AgentControlKind, AgentScheduleIssue,
    CHILD_RESULT_REQUIRED_FILES, CONTEXT_REQUIRED_DIRS, CONTEXT_REQUIRED_FILES, ChildContextStatus,
    ContextJsonlIssue, ContextJsonlKind, ContextPackIssue, ContextPackSourceError,
    DEFAULT_WORKER_MODEL, EventStreamIssue, MessageStreamIssue, ModelCapabilityIssue,
    ModelDriverRouteError, SESSION_REQUIRED_FILES, SessionControlKind, SessionIndexKind,
    ToolSchemaIssue, columnar, derive_agent_runtime_view, ensure_reference_tree,
    install_executable_object_wrapper, launch_process_for, parse_abi_path,
};
use std::cell::Cell;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};
include!("ctx/helpers.rs");
include!("ctx/output_mount.rs");
include!("ctx/parse_paths.rs");
include!("ctx/format_check.rs");
include!("ctx/file_doctor.rs");
include!("ctx/misc_terminal.rs");
include!("ctx/misc_socket.rs");
include!("ctx/session_gc.rs");
