#[test]
fn socket_session_recorder_appends_send_to_durable_history() {
    let root = clean_test_dir("socket-session-send");
    let session = root.join("default");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");

    let request = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
    );
    let request = ok!(request);
    let recorded = record_socket_request_to_session(&session, &request);
    let recorded = ok!(recorded);
    assert_eq!(recorded.messages().len(), 1);
    assert_eq!(recorded.events().len(), 1);

    let messages = fs::read_to_string(session.join("messages.jsonl"));
    let messages = ok!(messages);
    let events = fs::read_to_string(session.join("events.jsonl"));
    let events = ok!(events);
    assert!(inspect_message_stream_jsonl(&messages).is_ok());
    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(messages.contains("\"role\":\"user\""));
    assert!(messages.contains("\"content\":\"hello\""));
    assert!(events.contains("\"type\":\"start\""));
    assert_file_text(&session.join("state"), "active\n");
    assert_file_text(&session.join("cwd"), "/work/project\n");
}

#[test]
fn socket_session_recorder_revalidates_public_request_values() {
    let root = clean_test_dir("socket-session-revalidate");
    let session = root.join("default");
    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");

    let bad_send = SocketRequest::Send {
        id: "bad/id".to_owned(),
        session: "default".to_owned(),
        scope: SocketSessionScope::Private,
        cwd: Some("/work".to_owned()),
        input: "hello".to_owned(),
    };
    assert_eq!(
        record_socket_request_to_session(&session, &bad_send),
        Err(SocketSessionRecordError::InvalidField("id"))
    );
    let bad_cancel = SocketRequest::Cancel {
        id: "bad/id".to_owned(),
    };
    assert_eq!(
        record_socket_request_to_session(&session, &bad_cancel),
        Err(SocketSessionRecordError::InvalidField("id"))
    );
    assert_file_text(&session.join("messages.jsonl"), "");
    assert_file_text(&session.join("events.jsonl"), "");
}

#[test]
fn durable_session_layout_uses_private_modes_for_session_state() {
    let root = clean_test_dir("durable-session-private-modes");

    let result = ensure_durable_session_layout(
        &root,
        "default",
        "/work/project",
        Some("debug/echo"),
        SocketSessionScope::Private,
    );
    assert!(result.is_ok());

    let session = root.join("default");
    assert_eq!(
        fs::metadata(&session)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o700)
    );
    assert_eq!(
        fs::metadata(session.join("context"))
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o700)
    );
    for file in ["messages.jsonl", "events.jsonl", "meta.json", "cwd"] {
        assert_eq!(
            fs::metadata(session.join(file))
                .map(|metadata| metadata.permissions().mode() & 0o777)
                .ok(),
            Some(0o600),
            "{file}"
        );
    }
}

#[test]
fn durable_session_layout_rejects_session_symlink() {
    let root = clean_test_dir("durable-session-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let target = clean_test_dir("durable-session-symlink-target");
    assert!(fs::create_dir_all(&target).is_ok());
    assert!(symlink(&target, root.join("default")).is_ok());

    let result = ensure_durable_session_layout(
        &root,
        "default",
        "/work/project",
        Some("debug/echo"),
        SocketSessionScope::Private,
    );

    assert_eq!(result, Err(DurableSessionLayoutError::CannotCreate));
    assert!(!target.join("messages.jsonl").exists());
}

#[test]
fn durable_session_layout_rejects_symlink_required_file_without_writing_target() {
    let root = clean_test_dir("durable-session-file-symlink");
    let outside = clean_test_dir("durable-session-file-symlink-target");
    let session_root = root.join("session");
    let session = session_root.join("default");
    let target = outside.join("messages.jsonl");
    assert!(fs::create_dir_all(&session).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::write(&target, "outside\n").is_ok());
    assert!(symlink(&target, session.join("messages.jsonl")).is_ok());

    let result = ensure_durable_session_layout(
        &session_root,
        "default",
        "/work/project",
        Some("debug/echo"),
        SocketSessionScope::Private,
    );

    assert_eq!(result, Err(DurableSessionLayoutError::CannotCreate));
    assert_file_text(&target, "outside\n");
}

#[test]
fn durable_session_permission_helpers_repair_plain_file_and_dir_modes() {
    let root = clean_test_dir("durable-session-permission-repair");
    assert!(fs::create_dir_all(&root).is_ok());
    let file = root.join("state");
    let dir = root.join("context");
    write_text_file(&file, "idle\n");
    set_file_mode(&file, 0o644);
    assert!(fs::create_dir_all(&dir).is_ok());
    assert!(fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).is_ok());

    assert_eq!(set_text_file_permissions(&file), Ok(()));
    assert_eq!(set_private_dir_permissions(&dir), Ok(()));

    assert_eq!(
        fs::metadata(&file)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o600)
    );
    assert_eq!(
        fs::metadata(&dir)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o700)
    );
}

#[test]
fn durable_session_permission_helpers_refuse_symlinks_without_chmodding_targets() {
    let root = clean_test_dir("durable-session-permission-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("durable-session-permission-symlink-target");
    assert!(fs::create_dir_all(&outside).is_ok());
    let target_file = outside.join("state");
    let target_dir = outside.join("context");
    write_text_file(&target_file, "idle\n");
    set_file_mode(&target_file, 0o644);
    assert!(fs::create_dir_all(&target_dir).is_ok());
    assert!(fs::set_permissions(&target_dir, fs::Permissions::from_mode(0o755)).is_ok());
    let file_link = root.join("state");
    let dir_link = root.join("context");
    assert!(symlink(&target_file, &file_link).is_ok());
    assert!(symlink(&target_dir, &dir_link).is_ok());

    assert_eq!(
        set_text_file_permissions(&file_link),
        Err(DurableSessionLayoutError::CannotCreate)
    );
    assert_eq!(
        set_private_dir_permissions(&dir_link),
        Err(DurableSessionLayoutError::CannotCreate)
    );
    assert_eq!(
        fs::metadata(&target_file)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o644)
    );
    assert_eq!(
        fs::metadata(&target_dir)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o755)
    );
}

#[test]
fn durable_session_sync_plain_directory_refuses_symlink_without_touching_target() {
    let root = clean_test_dir("durable-session-sync-dir-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("durable-session-sync-dir-symlink-target");
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::set_permissions(&outside, fs::Permissions::from_mode(0o755)).is_ok());
    let link = root.join("context");
    assert!(symlink(&outside, &link).is_ok());

    let result = sync_plain_directory(&link);

    assert!(result.is_err());
    assert_eq!(
        fs::metadata(&outside)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o755)
    );
}

#[test]
fn write_text_file_if_absent_repairs_plain_file_mode_without_replacing_content() {
    let root = clean_test_dir("write-absent-existing-plain");
    assert!(fs::create_dir_all(&root).is_ok());
    let path = root.join("result.md");
    write_text_file(&path, "existing\n");
    set_file_mode(&path, 0o644);

    let result = write_text_file_if_absent(&path, "new\n");

    assert!(result.is_ok());
    assert_file_text(&path, "existing\n");
    assert_eq!(
        fs::metadata(&path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o600)
    );
}

#[test]
fn write_text_file_if_absent_refuses_symlink_without_chmodding_target() {
    let root = clean_test_dir("write-absent-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("write-absent-symlink-target");
    assert!(fs::create_dir_all(&outside).is_ok());
    let target = outside.join("target.md");
    write_text_file(&target, "target\n");
    set_file_mode(&target, 0o644);
    let link = root.join("result.md");
    assert!(symlink(&target, &link).is_ok());

    let result = write_text_file_if_absent(&link, "new\n");

    assert!(result.is_err());
    assert_file_text(&target, "target\n");
    assert_eq!(
        fs::metadata(&target)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o644)
    );
}

#[test]
fn write_text_file_if_absent_rejects_symlink_parent_without_writing_target() {
    let root = clean_test_dir("write-absent-symlink-parent");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("write-absent-symlink-parent-target");
    assert!(fs::create_dir_all(&outside).is_ok());
    let link = root.join("context");
    assert!(symlink(&outside, &link).is_ok());

    let result = write_text_file_if_absent(&link.join("result.md"), "new\n");

    assert!(result.is_err());
    assert!(!outside.join("result.md").exists());
}

#[test]
fn write_text_file_if_absent_rejects_symlink_parent_without_chmodding_existing_target() {
    let root = clean_test_dir("write-absent-symlink-parent-existing");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("write-absent-symlink-parent-existing-target");
    assert!(fs::create_dir_all(&outside).is_ok());
    let target = outside.join("result.md");
    write_text_file(&target, "target\n");
    set_file_mode(&target, 0o644);
    let link = root.join("context");
    assert!(symlink(&outside, &link).is_ok());

    let result = write_text_file_if_absent(&link.join("result.md"), "new\n");

    assert!(result.is_err());
    assert_file_text(&target, "target\n");
    assert_eq!(
        fs::metadata(&target)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o644)
    );
}

#[test]
fn write_text_file_if_absent_rejects_symlink_intermediate_parent_without_writing_target() {
    let root = clean_test_dir("write-absent-symlink-intermediate-parent");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("write-absent-symlink-intermediate-parent-target");
    assert!(fs::create_dir_all(outside.join("context")).is_ok());
    assert!(symlink(&outside, root.join("session")).is_ok());

    let result = write_text_file_if_absent(&root.join("session/context/result.md"), "new\n");

    assert!(result.is_err());
    assert!(!outside.join("context/result.md").exists());
}

#[test]
fn create_private_context_dir_repairs_plain_dir_mode() {
    let root = clean_test_dir("private-context-dir-existing");
    let path = root.join("context").join("child");
    assert!(fs::create_dir_all(&path).is_ok());
    assert!(fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).is_ok());

    let result = create_private_context_dir(&path);

    assert!(result.is_ok());
    assert_eq!(
        fs::metadata(&path)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o700)
    );
}

#[test]
fn create_private_context_dir_refuses_symlink_without_chmodding_target() {
    let root = clean_test_dir("private-context-dir-symlink");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("private-context-dir-symlink-target");
    let target = outside.join("target-dir");
    assert!(fs::create_dir_all(&target).is_ok());
    assert!(fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).is_ok());
    let link = root.join("child");
    assert!(symlink(&target, &link).is_ok());

    let result = create_private_context_dir(&link);

    assert!(result.is_err());
    assert_eq!(
        fs::metadata(&target)
            .map(|metadata| metadata.permissions().mode() & 0o777)
            .ok(),
        Some(0o755)
    );
}

#[test]
fn create_private_context_dir_rejects_symlink_parent_without_writing_target() {
    let root = clean_test_dir("private-context-dir-symlink-parent");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("private-context-dir-symlink-parent-target");
    assert!(fs::create_dir_all(&outside).is_ok());
    let link = root.join("context");
    assert!(symlink(&outside, &link).is_ok());

    let result = create_private_context_dir(&link.join("child"));

    assert!(result.is_err());
    assert!(!outside.join("child").exists());
}

#[test]
fn create_private_context_dir_rejects_symlink_intermediate_parent_without_writing_target() {
    let root = clean_test_dir("private-context-dir-symlink-intermediate-parent");
    assert!(fs::create_dir_all(&root).is_ok());
    let outside = clean_test_dir("private-context-dir-symlink-intermediate-parent-target");
    assert!(fs::create_dir_all(outside.join("context")).is_ok());
    assert!(symlink(&outside, root.join("session")).is_ok());

    let result = create_private_context_dir(&root.join("session/context/child"));

    assert!(result.is_err());
    assert!(!outside.join("context/child").exists());
}

#[test]
fn socket_session_recorder_rejects_symlink_required_files() {
    let root = clean_test_dir("socket-session-required-file-symlink");
    let session = root.join("default");
    let outside = clean_test_dir("socket-session-required-file-symlink-outside");
    create_complete_session_layout(&session);
    write_text_file(&outside.join("messages.jsonl"), "outside\n");
    assert!(fs::remove_file(session.join("messages.jsonl")).is_ok());
    assert!(symlink(
        outside.join("messages.jsonl"),
        session.join("messages.jsonl")
    )
    .is_ok());

    let request = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
    );
    let request = ok!(request);

    assert_eq!(
        record_socket_request_to_session(&session, &request),
        Err(SocketSessionRecordError::MissingSessionFile("messages.jsonl"))
    );
    assert_file_text(&outside.join("messages.jsonl"), "outside\n");
}

#[test]
fn socket_session_recorder_cancels_without_deleting_history() {
    let root = clean_test_dir("socket-session-cancel");
    let session = root.join("default");

    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"keep me\"}\n",
    );
    write_text_file(&session.join("events.jsonl"), "");

    let request = parse_socket_request_frame(r#"{"op":"cancel","id":"run-1"}"#);
    let request = ok!(request);
    let recorded = record_socket_request_to_session(&session, &request);
    let recorded = ok!(recorded);
    assert!(recorded.messages().is_empty());
    assert_eq!(recorded.events().len(), 1);

    let events = fs::read_to_string(session.join("events.jsonl"));
    let events = ok!(events);
    assert_file_text(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"keep me\"}\n",
    );
    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(events.contains("\"status\":\"cancelled\""));
    assert_file_text(&session.join("state"), "cancelled\n");
}

#[test]
fn assistant_response_recorder_updates_latest_without_replacing_history() {
    let root = clean_test_dir("assistant-response-record");
    let session = root.join("default");

    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"hello\"}\n",
    );
    write_text_file(
        &session.join("events.jsonl"),
        "{\"type\":\"start\",\"id\":\"run-1\",\"run\":\"run-1\"}\n",
    );
    write_text_file(&session.join("latest.md"), "old\n");

    let recorded = record_assistant_response_to_session(&session, "run-1", "hello back");
    let recorded = ok!(recorded);
    assert_eq!(recorded.messages().len(), 1);
    assert_eq!(recorded.events().len(), 2);

    let messages = fs::read_to_string(session.join("messages.jsonl"));
    let messages = ok!(messages);
    let events = fs::read_to_string(session.join("events.jsonl"));
    let events = ok!(events);
    assert!(inspect_message_stream_jsonl(&messages).is_ok());
    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(messages.contains("\"role\":\"user\""));
    assert!(messages.contains("\"role\":\"assistant\""));
    assert!(events.contains("\"type\":\"message\""));
    assert!(events.contains("\"status\":\"ok\""));
    assert_file_text(&session.join("latest.md"), "hello back\n");
    assert_file_text(&session.join("state"), "done\n");
}

#[test]
fn assistant_response_recorder_rejects_nul_content_without_recording() {
    let root = clean_test_dir("assistant-response-record-nul");
    let session = root.join("default");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session.join("latest.md"), "old\n");

    assert_eq!(
        record_assistant_response_to_session(&session, "run-1", "bad\0content"),
        Err(SocketSessionRecordError::InvalidField("content"))
    );
    assert_file_text(&session.join("messages.jsonl"), "");
    assert_file_text(&session.join("events.jsonl"), "");
    assert_file_text(&session.join("latest.md"), "old\n");
}

#[test]
fn tool_denial_recorder_makes_permission_failure_inspectable() {
    let root = clean_test_dir("tool-denial-record");
    let session = root.join("default");

    create_complete_session_layout(&session);
    write_text_file(
        &session.join("events.jsonl"),
        "{\"type\":\"start\",\"id\":\"run-1\",\"run\":\"run-1\"}\n",
    );

    let recorded = record_tool_execution_denial_to_session(
        &session,
        "run-1",
        "fs.read",
        ToolExecutionDenial::AgentPolicy,
    );
    let recorded = ok!(recorded);
    assert!(recorded.messages().is_empty());
    assert_eq!(recorded.events().len(), 2);

    let events = fs::read_to_string(session.join("events.jsonl"));
    let events = ok!(events);
    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(events.contains("\"type\":\"error\""));
    assert!(events.contains("\"tool\":\"fs.read\""));
    assert!(events.contains("\"code\":\"EACCES\""));
    assert!(events.contains("\"status\":\"error\""));
    assert_file_text(&session.join("state"), "error\n");
}

#[test]
fn tool_denial_recorder_rejects_invalid_tool_names() {
    let root = clean_test_dir("tool-denial-record-bad");
    let session = root.join("default");

    create_complete_session_layout(&session);

    assert_eq!(
        record_tool_execution_denial_to_session(
            &session,
            "run-1",
            "bad/tool",
            ToolExecutionDenial::InvalidToolName,
        ),
        Err(SocketSessionRecordError::InvalidField("tool"))
    );
}

#[test]
fn tool_result_recorder_appends_inspectable_tool_message_and_event() {
    let root = clean_test_dir("tool-result-record");
    let session = root.join("default");

    create_complete_session_layout(&session);
    write_text_file(
        &session.join("messages.jsonl"),
        "{\"role\":\"user\",\"content\":\"read README\"}\n",
    );
    write_text_file(
        &session.join("events.jsonl"),
        "{\"type\":\"start\",\"id\":\"run-1\",\"run\":\"run-1\"}\n",
    );

    let recorded = record_tool_execution_result_to_session(
        &session,
        "run-1",
        "call-1",
        "fs.read",
        "file contents",
    );
    let recorded = ok!(recorded);
    assert_eq!(recorded.messages().len(), 1);
    assert_eq!(recorded.events().len(), 1);

    let messages = fs::read_to_string(session.join("messages.jsonl"));
    let messages = ok!(messages);
    let events = fs::read_to_string(session.join("events.jsonl"));
    let events = ok!(events);

    assert!(inspect_message_stream_jsonl(&messages).is_ok());
    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(messages.contains("\"role\":\"tool\""));
    assert!(messages.contains("\"type\":\"tool_result\""));
    assert!(messages.contains("\"tool_call_id\":\"call-1\""));
    assert!(events.contains("\"name\":\"fs.read\""));
}

#[test]
fn tool_result_recorder_rejects_invalid_fields_without_executing() {
    let root = clean_test_dir("tool-result-record-bad");
    let session = root.join("default");

    create_complete_session_layout(&session);

    assert_eq!(
        record_tool_execution_result_to_session(&session, "run-1", "call-1", "bad/tool", "content",),
        Err(SocketSessionRecordError::InvalidField("tool"))
    );
    assert_eq!(
        record_tool_execution_result_to_session(
            &session,
            "run-1",
            "call-1",
            "fs.read",
            "bad\0content",
        ),
        Err(SocketSessionRecordError::InvalidField("content"))
    );
}

#[test]
fn socket_session_recorder_rejects_temp_resume_and_mismatched_sessions() {
    let root = clean_test_dir("socket-session-reject");
    let session = root.join("default");

    create_complete_session_layout(&session);

    let temp = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","scope":"temp","input":"hello"}"#,
    );
    let temp = ok!(temp);
    assert_eq!(
        record_socket_request_to_session(&session, &temp),
        Err(SocketSessionRecordError::TempSessionNotDurable)
    );

    let resume = parse_socket_request_frame(r#"{"op":"resume","session":"default"}"#);
    let resume = ok!(resume);
    assert_eq!(
        record_socket_request_to_session(&session, &resume),
        Err(SocketSessionRecordError::UnsupportedRequest)
    );

    let mismatch = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-2","session":"other","input":"hello"}"#,
    );
    let mismatch = ok!(mismatch);
    assert_eq!(
        record_socket_request_to_session(&session, &mismatch),
        Err(SocketSessionRecordError::SessionMismatch)
    );
    assert_eq!(SocketSessionRecordError::SessionMismatch.errno(), "EINVAL");
}

#[test]
fn indexed_socket_send_records_history_and_updates_session_index() {
    let root = clean_test_dir("indexed-socket-send");
    let session_root = root.join("session");
    let session = session_root.join("default");
    let previous = session_root.join("review-1");

    create_complete_session_layout(&session);
    create_complete_session_layout(&previous);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(
        &session_root.join("index").join("list"),
        "review-1\ndefault\n",
    );
    write_text_file(&session_root.join("index").join("current"), "review-1\n");
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());

    let request = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
    );
    let request = ok!(request);
    let recorded = record_indexed_socket_send_to_session(&session_root, &request);
    let recorded = ok!(recorded);
    assert_eq!(recorded.messages().len(), 1);
    assert_eq!(recorded.events().len(), 1);

    let by_cwd_key = session_index_key_for_cwd("/work/project");
    assert!(by_cwd_key.is_some());
    let Some(by_cwd_key) = by_cwd_key else { return };
    let messages = fs::read_to_string(session.join("messages.jsonl"));
    let messages = ok!(messages);
    let events = fs::read_to_string(session.join("events.jsonl"));
    let events = ok!(events);

    assert!(messages.contains("\"role\":\"user\""));
    assert!(events.contains("\"type\":\"start\""));
    assert_file_text(&session_root.join("index").join("list"), "default\nreview-1\n");
    assert_file_text(&session_root.join("index").join("current"), "default\n");
    assert_file_text(
        &session_root.join("index").join("by-cwd").join(by_cwd_key),
        "default\n",
    );
}

#[test]
fn indexed_socket_send_preflights_index_before_recording_history() {
    let root = clean_test_dir("indexed-socket-send-bad-index");
    let session_root = root.join("session");
    let session = session_root.join("default");

    create_complete_session_layout(&session);
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    write_text_file(&session_root.join("index").join("list"), "bad/name\n");
    write_text_file(&session_root.join("index").join("current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());

    let request = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
    );
    let request = ok!(request);

    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &request),
        Err(IndexedSocketSessionRecordError::Index(
            SessionIndexUpdateError::InvalidIndex
        ))
    );
    assert_file_text(&session.join("messages.jsonl"), "");
    assert_file_text(&session.join("events.jsonl"), "");
}

#[test]
fn indexed_socket_send_rejects_non_send_requests() {
    let root = clean_test_dir("indexed-socket-non-send");
    let session_root = root.join("session");


    let resume = parse_socket_request_frame(r#"{"op":"resume","session":"default"}"#);
    let resume = ok!(resume);
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &resume),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::UnsupportedRequest
        ))
    );

    let cancel = parse_socket_request_frame(r#"{"op":"cancel","id":"run-1"}"#);
    let cancel = ok!(cancel);
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &cancel),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::UnsupportedRequest
        ))
    );

    let ping = parse_socket_request_frame(r#"{"op":"ping"}"#);
    let ping = ok!(ping);
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &ping),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::UnsupportedRequest
        ))
    );
}
