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
    agent_bwrap_args, agent_native_tool_names, agent_new_request_json, agent_send_request_json,
    agent_repl_model_summary, agent_repl_prompt, agent_repl_editor_config,
    agent_repl_unknown_command_line,
    read_agent_repl_stdin_limited, current_session_name,
    latest_run_id, object_execution_command, parse_oauth_callback_params,
    read_oauth_callback_request_from_reader, read_optional_trimmed, read_provider_config_file, read_provider_config_from_dir,
    read_provider_secret_stdin_limited,
    remove_stale_agent_terminal_socket, run, AGENT_REPL_COMMANDS,
    agent_repl_should_exit_on_readline_error, read_file_to_string,
    agent_start_mounts_with_default_source, agent_start_process_command,
    agent_start_sandbox_cwd, agent_start_status_lines, agent_start_systemd_command,
    agent_terminal_socket,
    cli_error_line, cortexfs_xattr_line, socket_bind_path, terminal_socket_exists,
    visible_terminal_errno_is_best_effort, visible_terminal_write_error_is_best_effort,
    build_agent_system_prompt,
    AgentInterruptGuard, AgentProcess, collect_agent_events_buffered,
    collect_agent_events_buffered_interruptible, copy_socket_response_interruptible, ctx_state,
    cortexfs_mount_bin, ctx_root_entry_present, ctx_root_shape, env_exports, is_mount_point,
    plain_sibling_mount_bin, read_agent_processes, read_ctx_status, read_status_agent_processes,
    render_agent_event_lines, render_agent_process_tree, render_agent_status_lines,
    require_agent_mount, require_cli_name, require_session_name,
    classify_input_path, resolve_abi_path, run_visible_tool, run_visible_tool_with_writer, shell_quote_arg,
    stream_agent_socket_request_buffered_interruptible, stream_socket_request,
    stream_terminal_socket, terminal_connect_cli_error, AgentArgs, AgentMount,
    AgentStartArgs, AgentStartCommand, Cli, Command, FileCommand, LsTarget, ObjectClass,
    open_executable_no_follow, CliError,
    ProviderArgs, MAX_BUFFERED_AGENT_DIAGNOSTICS, MAX_BUFFERED_AGENT_EVENTS,
    MAX_BUFFERED_AGENT_RENDERED_BYTES, MAX_SOCKET_FRAME_BYTES, terminal_safe_text,
    systemctl_user_command, temp_file_name, waiting_diagnostic, debug_timing_diagnostic,
    CTX_PROVIDER_CURL_BIN,
    MAX_AGENT_REPL_STDIN_BYTES,
    MAX_OAUTH_CALLBACK_REQUEST_BYTES, MAX_PROVIDER_SECRET_STDIN_BYTES,
};
use cortexfs::{
    derive_agent_runtime_view, ensure_v1_reference_tree, parse_abi_path, AbiPathKind, AgentControlIssue, AgentControlKind,
    AgentScheduleIssue,
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
fn file_stat_xattr_line_escapes_terminal_controls() {
    let rendered = cortexfs_xattr_line("user.cortexfs.note\u{1b}[31m", "ok\u{7}");

    assert_eq!(rendered, "xattr.user.cortexfs.note\\u{1b}[31m=ok\\u{7}");
    assert!(!rendered.as_bytes().contains(&0x1b));
    assert!(!rendered.as_bytes().contains(&0x07));
}

#[test]
fn cli_error_line_escapes_terminal_controls() {
    let rendered = cli_error_line(&CliError::usage(
        "unexpected argument: --bad\u{1b}]52;c;payload\u{7}",
    ));

    assert_eq!(
        rendered,
        "ctx: unexpected argument: --bad\\u{1b}]52;c;payload\\u{7}"
    );
    assert!(!rendered.as_bytes().contains(&0x1b));
    assert!(!rendered.as_bytes().contains(&0x07));
}

#[test]
fn agent_repl_unknown_command_line_escapes_terminal_controls() {
    let rendered = agent_repl_unknown_command_line("/bad\u{1b}]52;c;payload\u{7}");

    assert_eq!(
        rendered,
        "ctx: unknown repl command: /bad\\u{1b}]52;c;payload\\u{7}"
    );
    assert!(!rendered.as_bytes().contains(&0x1b));
    assert!(!rendered.as_bytes().contains(&0x07));
}

#[test]
fn temp_file_name_changes_with_retry_attempt() {
    assert_ne!(temp_file_name(0), temp_file_name(1));
}

#[test]
fn provider_config_file_reader_refuses_symlink_targets() {
    let root = clean_test_dir("ctx-provider-config-reader-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let target = root.join("target.json");
    let link = root.join("local.json");
    assert!(fs::write(&target, "{\"base_url\":\"http://127.0.0.1:8317/v1\"}\n").is_ok());
    assert!(std::os::unix::fs::symlink(&target, &link).is_ok());

    assert!(read_provider_config_file(&link).is_err());
}

#[test]
fn ctx_executable_open_refuses_symlink_targets() {
    let root = clean_test_dir("ctx-executable-open-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let target = root.join("target");
    let link = root.join("tool");
    assert!(fs::write(&target, "#!/bin/sh\nexit 0\n").is_ok());
    assert!(fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).is_ok());
    assert!(std::os::unix::fs::symlink(&target, &link).is_ok());

    assert!(open_executable_no_follow(&link).is_err());
}

#[test]
fn provider_config_reader_lists_plain_config_dir() {
    let root = clean_test_dir("ctx-provider-config-reader-dir");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::write(
        root.join("local.json"),
        "{\"name\":\"local\",\"base_url\":\"http://127.0.0.1:8317/v1\"}\n",
    )
    .is_ok());

    assert!(read_provider_config_from_dir("local", &root).is_ok());
}

#[test]
fn provider_config_reader_rejects_symlink_config_dir() {
    let root = clean_test_dir("ctx-provider-config-reader-dir-symlink");
    let outside = clean_test_dir("ctx-provider-config-reader-dir-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::write(
        outside.join("local.json"),
        "{\"name\":\"local\",\"base_url\":\"http://127.0.0.1:8317/v1\"}\n",
    )
    .is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("providers.d")).is_ok());

    assert!(read_provider_config_from_dir("local", &root.join("providers.d")).is_err());
}

#[test]
fn provider_secret_stdin_reader_accepts_input_at_limit() {
    let secret = "x".repeat(MAX_PROVIDER_SECRET_STDIN_BYTES);

    let read = read_provider_secret_stdin_limited(
        std::io::Cursor::new(secret.as_bytes()),
        MAX_PROVIDER_SECRET_STDIN_BYTES,
    );

    assert_eq!(read.unwrap_or_default().len(), MAX_PROVIDER_SECRET_STDIN_BYTES);
}

#[test]
fn provider_secret_stdin_reader_rejects_input_over_limit() {
    let secret = "x".repeat(MAX_PROVIDER_SECRET_STDIN_BYTES + 1);

    let read = read_provider_secret_stdin_limited(
        std::io::Cursor::new(secret.as_bytes()),
        MAX_PROVIDER_SECRET_STDIN_BYTES,
    );

    assert!(matches!(read, Err(ref error) if error.kind() == std::io::ErrorKind::InvalidData));
}

#[test]
fn agent_repl_stdin_reader_accepts_input_at_limit() {
    let input = "x".repeat(MAX_AGENT_REPL_STDIN_BYTES);

    let read = read_agent_repl_stdin_limited(
        std::io::Cursor::new(input.as_bytes()),
        MAX_AGENT_REPL_STDIN_BYTES,
    );

    assert_eq!(read.unwrap_or_default().len(), MAX_AGENT_REPL_STDIN_BYTES);
}

#[test]
fn agent_repl_stdin_reader_rejects_input_over_limit() {
    let input = "x".repeat(MAX_AGENT_REPL_STDIN_BYTES + 1);

    let read = read_agent_repl_stdin_limited(
        std::io::Cursor::new(input.as_bytes()),
        MAX_AGENT_REPL_STDIN_BYTES,
    );

    assert!(matches!(read, Err(ref error) if error.kind() == std::io::ErrorKind::InvalidData));
}

#[test]
fn provider_config_file_reader_refuses_symlink_intermediate_directory() {
    let root = clean_test_dir("ctx-provider-config-reader-intermediate-symlink");
    let outside = clean_test_dir("ctx-provider-config-reader-intermediate-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("providers.d")).is_ok());
    assert!(fs::write(
        outside.join("providers.d/local.json"),
        "{\"base_url\":\"http://127.0.0.1:8317/v1\"}\n",
    )
    .is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("etc")).is_ok());

    assert!(read_provider_config_file(&root.join("etc/providers.d/local.json")).is_err());
}

#[test]
fn provider_config_atomic_write_replaces_file_without_fixed_temp_name() {
    let root = clean_test_dir("ctx-provider-config-atomic-write");
    assert!(fs::create_dir_all(&root).is_ok());
    let path = root.join("local.json");

    assert!(atomic_write_provider_config(&path, "{\"base_url\":\"http://old/v1\"}\n").is_ok());
    assert_eq!(
        fs::read_to_string(&path).unwrap_or_default(),
        "{\"base_url\":\"http://old/v1\"}\n"
    );
    assert!(atomic_write_provider_config(&path, "{\"base_url\":\"http://new/v1\"}\n").is_ok());
    assert_eq!(
        fs::read_to_string(&path).unwrap_or_default(),
        "{\"base_url\":\"http://new/v1\"}\n"
    );
    assert_eq!(
        fs::metadata(&path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o600)
    );
    assert!(!root.join("local.json.tmp").exists());
}

#[test]
fn provider_config_atomic_write_rejects_symlink_parent_directory() {
    let root = clean_test_dir("ctx-provider-config-atomic-write-parent-symlink");
    let outside = clean_test_dir("ctx-provider-config-atomic-write-outside");
    assert!(fs::create_dir_all(root.join("etc")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("etc/providers.d")).is_ok());

    let path = root.join("etc/providers.d/local.json");

    assert!(atomic_write_provider_config(&path, "{\"base_url\":\"http://new/v1\"}\n").is_err());
    assert!(!outside.join("local.json").exists());
    assert!(!fs::read_dir(&outside).map_or(true, |entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp")
        })
    }));
}

#[test]
fn provider_config_atomic_write_rejects_symlink_intermediate_directory() {
    let root = clean_test_dir("ctx-provider-config-atomic-write-intermediate-symlink");
    let outside = clean_test_dir("ctx-provider-config-atomic-write-intermediate-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("providers.d")).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("etc")).is_ok());

    let path = root.join("etc/providers.d/local.json");

    assert!(atomic_write_provider_config(&path, "{\"base_url\":\"http://new/v1\"}\n").is_err());
    assert!(!outside.join("providers.d/local.json").exists());
    assert!(!fs::read_dir(outside.join("providers.d")).map_or(true, |entries| {
        entries.filter_map(Result::ok).any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(".tmp")
        })
    }));
}

#[test]
fn ctx_file_helpers_refuse_symlink_reads_and_appends() {
    let root = clean_test_dir("ctx-file-symlink-io");
    assert!(fs::create_dir_all(&root).is_ok());
    let target = root.join("outside.txt");
    let link = root.join("link.txt");
    assert!(fs::write(&target, "outside\n").is_ok());
    assert!(std::os::unix::fs::symlink(&target, &link).is_ok());

    assert!(cat_path(&link).is_err());
    assert!(read_file_to_string(&link).is_err());
    assert!(file_append(&root, "link.txt", "changed").is_err());
    assert_eq!(fs::read_to_string(&target).unwrap_or_default(), "outside\n");
}

#[test]
fn ctx_file_helpers_refuse_symlink_intermediate_reads() {
    let root = clean_test_dir("ctx-file-symlink-intermediate-read");
    let outside = clean_test_dir("ctx-file-symlink-intermediate-read-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("session")).is_ok());
    assert!(fs::write(outside.join("session/state"), "outside\n").is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("link")).is_ok());
    let path = root.join("link/session/state");

    assert!(cat_path(&path).is_err());
    assert!(read_file_to_string(&path).is_err());
}

#[test]
fn ctx_file_type_refuses_symlink_intermediate_path() {
    let root = clean_test_dir("ctx-file-type-symlink-intermediate");
    let outside = clean_test_dir("ctx-file-type-symlink-intermediate-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("session")).is_ok());
    assert!(fs::write(outside.join("session/state"), "outside\n").is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("link")).is_ok());

    assert!(file_type_name(&root, "link/session/state").is_err());
}

#[test]
fn ctx_file_writes_reject_symlink_parent_without_writing_target() {
    let root = clean_test_dir("ctx-file-symlink-parent-write");
    let outside = clean_test_dir("ctx-file-symlink-parent-write-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("link")).is_ok());

    assert!(file_set(&root, "link/state", "changed").is_err());
    assert!(file_append(&root, "link/events.jsonl", "{\"type\":\"changed\"}").is_err());
    assert!(!outside.join("state").exists());
    assert!(!outside.join("events.jsonl").exists());
}

#[test]
fn ctx_file_writes_reject_symlink_intermediate_parent_without_writing_target() {
    let root = clean_test_dir("ctx-file-symlink-intermediate-write");
    let outside = clean_test_dir("ctx-file-symlink-intermediate-write-outside");
    let outside_session = outside.join("session");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside_session).is_ok());
    assert!(fs::write(outside_session.join("state"), "outside\n").is_ok());
    assert!(fs::write(outside_session.join("events.jsonl"), "outside\n").is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("link")).is_ok());

    assert!(file_set(&root, "link/session/state", "changed").is_err());
    assert!(file_append(&root, "link/session/events.jsonl", "{\"type\":\"changed\"}").is_err());
    assert_eq!(
        fs::read_to_string(outside_session.join("state")).unwrap_or_default(),
        "outside\n"
    );
    assert_eq!(
        fs::read_to_string(outside_session.join("events.jsonl")).unwrap_or_default(),
        "outside\n"
    );
}

#[test]
fn ctx_agent_read_helpers_refuse_symlink_files() {
    let root = clean_test_dir("ctx-agent-read-helper-symlink");
    let outside = clean_test_dir("ctx-agent-read-helper-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    write_text_file(&outside.join("model"), "outside\n");
    assert!(std::os::unix::fs::symlink(outside.join("model"), root.join("model")).is_ok());

    assert!(read_optional_trimmed(&root.join("model")).is_err());
}

#[test]
fn ctx_latest_run_id_refuses_symlink_events() {
    let root = clean_test_dir("ctx-latest-run-id-symlink-events");
    let outside = clean_test_dir("ctx-latest-run-id-symlink-events-outside");
    let session = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session")
        .join("default");
    create_complete_session_layout(&session);
    write_text_file(&outside.join("events.jsonl"), "{\"type\":\"start\",\"run\":\"outside\"}\n");
    assert!(fs::remove_file(session.join("events.jsonl")).is_ok());
    assert!(std::os::unix::fs::symlink(
        outside.join("events.jsonl"),
        session.join("events.jsonl")
    )
    .is_ok());

    assert!(latest_run_id(&root, "coder", "default").is_err());
    assert_eq!(
        fs::read_to_string(outside.join("events.jsonl")).unwrap_or_default(),
        "{\"type\":\"start\",\"run\":\"outside\"}\n"
    );
}

#[test]
fn agent_terminal_runtime_dir_refuses_symlink_parent() {
    let root = clean_test_dir("ctx-agent-terminal-runtime-dir-symlink");
    let outside = clean_test_dir("ctx-agent-terminal-runtime-dir-outside");
    assert!(fs::create_dir_all(root.join("cortexfs")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("cortexfs").join("terminal")).is_ok());

    let path = root.join("cortexfs").join("terminal").join("coder").join("default");

    assert!(create_agent_terminal_runtime_dir(&path).is_err());
    assert!(!outside.join("coder").exists());
    assert!(
        root.join("cortexfs")
            .join("terminal")
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    );
}

#[test]
fn ensure_agent_terminal_socket_rejects_symlink_runtime_parent() {
    let root = clean_test_dir("ctx-agent-terminal-socket-runtime-symlink");
    let outside = clean_test_dir("ctx-agent-terminal-socket-runtime-symlink-outside");
    assert!(fs::create_dir_all(root.join("runtime").join("cortexfs")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(
        &outside,
        root.join("runtime").join("cortexfs").join("terminal")
    )
    .is_ok());
    let visible_socket = root.join("visible").join("main.sock");
    let runtime_socket = root
        .join("runtime")
        .join("cortexfs")
        .join("terminal")
        .join("coder")
        .join("default")
        .join("main.sock");

    assert!(ensure_agent_terminal_socket(&visible_socket, &runtime_socket).is_err());
    assert!(!outside.join("coder").exists());
    assert!(!visible_socket.exists());
}

#[test]
fn ensure_agent_terminal_socket_rejects_symlink_runtime_parent_with_existing_target_dirs() {
    let root = clean_test_dir("ctx-agent-terminal-socket-runtime-existing-symlink");
    let outside = clean_test_dir("ctx-agent-terminal-socket-runtime-existing-symlink-outside");
    assert!(fs::create_dir_all(root.join("runtime").join("cortexfs")).is_ok());
    assert!(fs::create_dir_all(outside.join("coder").join("default")).is_ok());
    assert!(std::os::unix::fs::symlink(
        &outside,
        root.join("runtime").join("cortexfs").join("terminal")
    )
    .is_ok());
    let visible_socket = root.join("visible").join("main.sock");
    let runtime_socket = root
        .join("runtime")
        .join("cortexfs")
        .join("terminal")
        .join("coder")
        .join("default")
        .join("main.sock");

    assert!(ensure_agent_terminal_socket(&visible_socket, &runtime_socket).is_err());
    assert!(!outside
        .join("coder")
        .join("default")
        .join(".empty-shell-startup")
        .exists());
    assert!(!visible_socket.exists());
}

#[test]
fn ensure_agent_terminal_socket_rejects_symlink_visible_parent_without_writing_target() {
    let root = clean_test_dir("ctx-agent-terminal-socket-visible-symlink");
    let outside = clean_test_dir("ctx-agent-terminal-socket-visible-symlink-outside");
    assert!(fs::create_dir_all(root.join("runtime")).is_ok());
    assert!(fs::create_dir_all(root.join("visible")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("visible").join("terminal")).is_ok());
    let visible_socket = root.join("visible").join("terminal").join("main.sock");
    let runtime_socket = root.join("runtime").join("main.sock");

    assert!(ensure_agent_terminal_socket(&visible_socket, &runtime_socket).is_err());
    assert!(!outside.join("main.sock").exists());
    assert!(root
        .join("visible")
        .join("terminal")
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink()));
}

#[test]
fn remove_stale_agent_terminal_socket_refuses_plain_file() {
    let root = clean_test_dir("ctx-agent-terminal-remove-plain-file");
    assert!(fs::create_dir_all(&root).is_ok());
    let socket = root.join("main.sock");
    write_text_file(&socket, "keep\n");

    assert!(remove_stale_agent_terminal_socket(&socket).is_err());
    assert_eq!(fs::read_to_string(&socket).unwrap_or_default(), "keep\n");
}

#[test]
fn remove_stale_agent_terminal_socket_rejects_symlink_parent_without_removing_target_socket()
-> Result<(), Box<dyn std::error::Error>> {
    let root = clean_test_dir("ctx-agent-terminal-remove-parent-symlink");
    let outside = clean_test_dir("ctx-agent-terminal-remove-parent-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    let outside_socket = outside.join("main.sock");
    let listener = std::os::unix::net::UnixListener::bind(&outside_socket)?;
    assert!(std::os::unix::fs::symlink(&outside, root.join("runtime")).is_ok());

    let Err(error) = remove_stale_agent_terminal_socket(&root.join("runtime").join("main.sock"))
    else {
        return Err("symlink parent must fail".into());
    };

    assert!(matches!(
        error.kind(),
        std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::NotADirectory
    ));
    assert!(outside_socket.exists());
    drop(listener);
    Ok(())
}

#[test]
fn terminal_socket_exists_rejects_plain_file() {
    let root = clean_test_dir("ctx-terminal-socket-exists-plain-file");
    let socket = root.join("main.sock");
    write_text_file(&socket, "not a socket\n");

    assert!(!terminal_socket_exists(&socket));
}

#[test]
fn socket_bind_path_rejects_symlink_parent() {
    let root = clean_test_dir("ctx-terminal-socket-bind-parent-symlink");
    let outside = clean_test_dir("ctx-terminal-socket-bind-parent-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("runtime")).is_ok());
    assert!(std::os::unix::fs::symlink(
        outside.join("runtime").join("main.sock"),
        outside.join("main.sock")
    )
    .is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("visible")).is_ok());

    let socket = root.join("visible").join("main.sock");

    assert_eq!(socket_bind_path(&socket), socket);
}

#[test]
fn provider_oauth_uses_absolute_curl_path() {
    let command = ctx_provider_curl_command();
    assert_eq!(command.get_program(), CTX_PROVIDER_CURL_BIN);
    assert_eq!(
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["-q", "--config", "-"]
    );
    assert!(command.get_envs().next().is_none());
}

#[test]
fn provider_oauth_curl_quote_rejects_line_breaks() {
    assert!(curl_config_quote("https://oauth.example/token").is_ok());
    assert!(curl_config_quote("https://oauth.example/token\noutput = /tmp/leak").is_err());
    assert!(curl_config_quote("grant_type=refresh_token\rheader = injected").is_err());
    assert!(curl_config_quote("Authorization: Bearer \u{1b}]52;c;payload").is_err());
    assert!(curl_config_quote("abc\0def").is_err());
}

#[test]
fn oauth_callback_reader_stops_after_headers() {
    let request = b"GET /callback?code=ok HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\nignored body";

    let read = read_oauth_callback_request_from_reader(
        std::io::Cursor::new(request.as_slice()),
        MAX_OAUTH_CALLBACK_REQUEST_BYTES,
    );

    assert_eq!(
        read.unwrap_or_default(),
        "GET /callback?code=ok HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
    );
}

#[test]
fn oauth_callback_reader_rejects_oversized_headers() {
    let request = vec![b'a'; MAX_OAUTH_CALLBACK_REQUEST_BYTES + 1];

    let read = read_oauth_callback_request_from_reader(
        std::io::Cursor::new(request),
        MAX_OAUTH_CALLBACK_REQUEST_BYTES,
    );

    assert!(matches!(read, Err(ref error) if error.code == 69));
}

#[test]
fn oauth_callback_parser_requires_http_version() {
    let parsed = parse_oauth_callback_params("GET /callback?code=ok&state=s\n\n", "/callback");

    assert!(matches!(parsed, Err(ref error) if error.code == 2));
}

#[test]
fn oauth_callback_parser_rejects_extra_request_line_fields() {
    let parsed =
        parse_oauth_callback_params("GET /callback?code=ok&state=s HTTP/1.1 extra\n\n", "/callback");

    assert!(matches!(parsed, Err(ref error) if error.code == 2));
}

#[test]
fn oauth_callback_parser_accepts_http_1_request_line() {
    let parsed =
        parse_oauth_callback_params("GET /callback?code=ok&state=s HTTP/1.1\r\n\r\n", "/callback");

    assert!(matches!(
        parsed,
        Ok(ref params)
            if params.code.as_deref() == Some("ok")
                && params.state.as_deref() == Some("s")
    ));
}

#[test]
fn oauth_callback_parser_rejects_repeated_code() {
    let parsed =
        parse_oauth_callback_params("GET /callback?code=one&code=two HTTP/1.1\r\n\r\n", "/callback");

    assert!(matches!(parsed, Err(ref error) if error.code == 2));
}

#[test]
fn oauth_callback_parser_rejects_repeated_state() {
    let parsed = parse_oauth_callback_params(
        "GET /callback?code=ok&state=one&state=two HTTP/1.1\r\n\r\n",
        "/callback",
    );

    assert!(matches!(parsed, Err(ref error) if error.code == 2));
}

#[test]
fn oauth_callback_parser_rejects_empty_code() {
    let parsed =
        parse_oauth_callback_params("GET /callback?code=&state=s HTTP/1.1\r\n\r\n", "/callback");

    assert!(matches!(parsed, Err(ref error) if error.code == 2));
}

#[test]
fn oauth_callback_parser_rejects_empty_state() {
    let parsed =
        parse_oauth_callback_params("GET /callback?code=ok&state= HTTP/1.1\r\n\r\n", "/callback");

    assert!(matches!(parsed, Err(ref error) if error.code == 2));
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

#[test]
fn current_session_name_rejects_symlink_index_dir_without_reading_target() {
    let root = clean_test_dir("ctx-current-session-symlink-index");
    let outside = clean_test_dir("ctx-current-session-symlink-index-target");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::write(outside.join("current"), "outside\n").is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("index")).is_ok());

    let result = current_session_name(&root);

    assert!(result.is_err());
}

#[test]
fn current_session_name_rejects_symlink_current_file_without_reading_target() {
    let root = clean_test_dir("ctx-current-session-symlink-current");
    let outside = clean_test_dir("ctx-current-session-symlink-current-target");
    let index = root.join("index");
    assert!(fs::create_dir_all(&index).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::write(outside.join("current"), "outside\n").is_ok());
    assert!(std::os::unix::fs::symlink(outside.join("current"), index.join("current")).is_ok());

    let result = current_session_name(&root);

    assert!(result.is_err());
}
