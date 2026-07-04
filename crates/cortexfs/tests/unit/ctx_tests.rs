use super::{
    agent_lifecycle_tool_command, atomic_write_provider_config, cat_path,
    create_agent_terminal_runtime_dir, create_plain_mountpoint_dir, ctx_provider_curl_command, curl_config_quote,
    detached_mount_command, direct_mount_command, doctor, doctor_report_line, doctor_root_line,
    doctor_unexpected_entry_line, ensure_agent_terminal_socket,
    ensure_plain_mountpoint_dir, file_append, file_check, file_set, file_type_name,
    format_agent_control_issues, format_agent_schedule_issues, format_context_jsonl_issues,
    format_context_pack_issues, format_event_stream_issues, format_message_stream_issues,
    format_model_capability_issues, format_model_driver_route_error, format_object_layout_issues,
    format_session_control_issues, format_session_index_issues, format_session_layout_issues,
    format_shared_queue_layout_issues, format_tool_schema_issues, format_debug_tool_line,
    json_string, list_names, newline_terminated, parse, parse_command, absolute_existing_path,
    ctx_home,
    agent_bwrap_args, agent_chat_runtime_socket, agent_chat_socket_systemd_command,
    agent_chat_unit, agent_host_mount_source, agent_native_tool_names, agent_new,
    agent_new_request_json, agent_send_request_json,
    agent_repl_banner_lines, agent_repl_model_summary, agent_repl_prompt, agent_repl_editor_config,
    agent_repl_unknown_command_line,
    read_agent_repl_stdin_limited, current_session_name,
    latest_run_id, object_execution_command, parse_oauth_callback_params,
    read_oauth_callback_request_from_reader, read_optional_trimmed, read_provider_config_file, read_provider_config_from_dir,
    read_provider_secret_stdin_limited,
    remove_stale_agent_terminal_socket, run, load_schedule_context, schedule_command,
    schedule_handoff_agent_details,
    schedule_parent_ref_for_output, schedule_require_handoff_parent, schedule_status_lines, AGENT_REPL_COMMANDS,
    agent_env_lines, agent_repl_should_exit_on_readline_error, read_file_to_string,
    agent_start_mounts_with_default_source, agent_start_process_command,
    agent_start_sandbox_cwd, agent_start_status_lines, agent_start_systemd_command,
    record_agent_start_state,
    agent_child_rows, agent_status_lines, agent_stop, agent_terminal_socket,
    agent_terminal_units, agent_wait,
    cli_error_line, cortexfs_xattr_line, socket_bind_path, terminal_socket_exists,
    visible_terminal_errno_is_best_effort, visible_terminal_write_error_is_best_effort,
    build_agent_system_prompt,
    AgentInterruptGuard, AgentProcess, collect_agent_events_buffered,
    collect_agent_events_buffered_interruptible, copy_socket_response_interruptible, ctx_state,
    cortexfs_mount_bin, ctx_root_entry_present, ctx_root_shape, env_exports, is_mount_point,
    parse_systemctl_main_pid, plain_sibling_mount_bin, read_agent_processes, read_ctx_status,
    read_status_agent_processes,
    render_agent_event_lines, render_agent_process_tree, render_agent_status_lines,
    require_agent_mount, require_cli_name, require_session_name,
    classify_input_path, resolve_abi_path, run_visible_tool, run_visible_tool_with_writer,
    schedule_child_context_abi_paths, schedule_context_abi_path, shell_quote_arg,
    stream_agent_socket_request_buffered_interruptible, stream_socket_request,
    stream_terminal_socket, terminal_connect_cli_error, AgentArgs, AgentChildRow, AgentMount,
    AgentStartArgs, AgentStartCommand, Cli, Command, FileCommand, LsTarget, ObjectClass,
    open_executable_no_follow, CliError,
    ProviderArgs, ScheduleArgs, ScheduleChildContextAbiPaths, MAX_AGENT_EVENTS,
    MAX_AGENT_RESPONSE_BYTES, MAX_BUFFERED_AGENT_DIAGNOSTICS, MAX_BUFFERED_AGENT_EVENTS,
    MAX_BUFFERED_AGENT_RENDERED_BYTES, MAX_SOCKET_FRAME_BYTES, terminal_safe_text,
    systemctl_user_command, temp_file_name, waiting_diagnostic, debug_timing_diagnostic,
    CTX_PROVIDER_CURL_BIN,
    MAX_AGENT_REPL_STDIN_BYTES,
    MAX_OAUTH_CALLBACK_REQUEST_BYTES, MAX_PROVIDER_SECRET_STDIN_BYTES,
};
use cortexfs::{
    AGENT_CONTROL_FILES,
    DEFAULT_WORKER_MODEL, derive_agent_runtime_view, ensure_v1_reference_tree,
    install_executable_object_wrapper, parse_abi_path, AbiPathKind, AgentControlIssue, AgentControlKind,
    AgentScheduleIssue,
    ContextJsonlIssue, ContextJsonlKind, ContextPackIssue, ContextPackSourceError, EventStreamIssue,
    MessageStreamIssue, ModelCapabilityIssue, ModelDriverRouteError, ObjectLayoutIssue,
    SessionControlIssue, SessionControlKind, SessionIndexIssue, SessionIndexKind,
    SessionLayoutIssue, SharedQueueLayoutIssue, ToolSchemaIssue, ChildContextStatus,
    CHILD_RESULT_REQUIRED_FILES, CONTEXT_REQUIRED_DIRS, CONTEXT_REQUIRED_FILES,
    SESSION_REQUIRED_FILES,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
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
