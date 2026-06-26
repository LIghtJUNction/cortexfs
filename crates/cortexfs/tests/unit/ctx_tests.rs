use super::{
    doctor, file_check,
    format_agent_control_issues, format_context_jsonl_issues, format_context_pack_issues,
    format_event_stream_issues, format_message_stream_issues, format_model_capability_issues,
    format_model_driver_route_error, format_object_layout_issues, format_session_control_issues,
    format_session_index_issues, format_session_layout_issues, format_shared_queue_layout_issues,
    format_tool_schema_issues, format_debug_tool_line, json_string, list_names, newline_terminated, parse_command,
    absolute_existing_path, agent_bwrap_args, agent_new_request_json, agent_repl_model_summary,
    agent_repl_prompt, agent_repl_editor_config, current_session_name, run,
    agent_repl_should_exit_on_readline_error,
    agent_start_mounts_with_default_source, agent_start_status_lines, agent_start_systemd_command,
    agent_terminal_socket, build_agent_system_prompt,
    AgentInterruptGuard, collect_agent_events_buffered,
    collect_agent_events_buffered_interruptible, copy_socket_response_interruptible, ctx_state,
    env_exports, is_mount_point, read_agent_processes, read_ctx_status, read_status_agent_processes,
    render_agent_process_tree, render_agent_status_lines, require_agent_mount, require_cli_name,
    require_session_name,
    resolve_abi_path, run_visible_tool, run_visible_tool_with_writer, shell_quote_arg, stream_agent_socket_request_buffered_interruptible, stream_socket_request,
    stream_terminal_socket, terminal_connect_cli_error, AgentArgs, AgentMount,
    AgentStartArgs, Command, FileCommand, LsTarget, ObjectClass,
    ProviderArgs, MAX_BUFFERED_AGENT_DIAGNOSTICS, MAX_BUFFERED_AGENT_EVENTS,
    MAX_BUFFERED_AGENT_RENDERED_BYTES, MAX_SOCKET_FRAME_BYTES, terminal_safe_text,
};
use cortexfs::{
    derive_agent_runtime_view, ensure_v1_reference_tree, parse_abi_path, AbiPathKind, AgentControlIssue, AgentControlKind,
    ContextJsonlIssue, ContextJsonlKind, ContextPackIssue, ContextPackSourceError, EventStreamIssue,
    MessageStreamIssue, ModelCapabilityIssue, ModelDriverRouteError, ObjectLayoutIssue,
    SessionControlIssue, SessionControlKind, SessionIndexIssue, SessionIndexKind,
    SessionLayoutIssue, SharedQueueLayoutIssue, ToolSchemaIssue, CHILD_RESULT_REQUIRED_FILES,
    CONTEXT_REQUIRED_DIRS, CONTEXT_REQUIRED_FILES, SESSION_REQUIRED_FILES,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
include!("ctx/helpers.rs");
include!("ctx/output_mount.rs");
include!("ctx/parse_paths.rs");
include!("ctx/format_check.rs");
include!("ctx/file_doctor.rs");

#[test]
fn terminal_safe_text_escapes_control_sequences() {
    let malicious = "prefix\u{1b}]52;c;payload\u{7}suffix";

    let rendered = terminal_safe_text(malicious);

    assert_eq!(rendered, "prefix\\u{1b}]52;c;payload\\u{7}suffix");
    assert!(!rendered.as_bytes().contains(&0x1b));
    assert!(!rendered.as_bytes().contains(&0x07));
}

#[test]
fn terminal_safe_text_preserves_common_formatting() {
    assert_eq!(terminal_safe_text("a\nb\tc\rd"), "a\nb\tc\rd");
}

#[test]
fn current_session_name_falls_back_to_default_when_index_is_unreadable() {
    let root = clean_test_dir("ctx-current-session-unreadable");
    let index = root.join("index");
    assert!(fs::create_dir_all(&index).is_ok(), "failed to create session index directory");
    let current = index.join("current");
    assert!(fs::write(&current, "custom\n").is_ok(), "failed to write current session override");
    assert!(
        fs::set_permissions(&current, fs::Permissions::from_mode(0o000)).is_ok(),
        "failed to set current session file unreadable"
    );

    let result = current_session_name(&root);

    assert!(
        fs::set_permissions(&current, fs::Permissions::from_mode(0o600)).is_ok(),
        "failed to restore current session file permissions"
    );
    let Ok(name) = result else {
        return;
    };
    assert_eq!(name, "default");
}
