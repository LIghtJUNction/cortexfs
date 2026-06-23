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
    let state = fs::read_to_string(session.join("state"));
    let state = ok!(state);
    assert_eq!(state, "active\n");
    let cwd = fs::read_to_string(session.join("cwd"));
    let cwd = ok!(cwd);
    assert_eq!(cwd, "/work/project\n");

    let _ignored = fs::remove_dir_all(&root);
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

    let messages = fs::read_to_string(session.join("messages.jsonl"));
    let messages = ok!(messages);
    let events = fs::read_to_string(session.join("events.jsonl"));
    let events = ok!(events);
    assert_eq!(messages, "{\"role\":\"user\",\"content\":\"keep me\"}\n");
    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(events.contains("\"status\":\"cancelled\""));
    let state = fs::read_to_string(session.join("state"));
    let state = ok!(state);
    assert_eq!(state, "cancelled\n");

    let _ignored = fs::remove_dir_all(&root);
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
    let latest = fs::read_to_string(session.join("latest.md"));
    let latest = ok!(latest);
    let state = fs::read_to_string(session.join("state"));
    let state = ok!(state);

    assert!(inspect_message_stream_jsonl(&messages).is_ok());
    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(messages.contains("\"role\":\"user\""));
    assert!(messages.contains("\"role\":\"assistant\""));
    assert!(events.contains("\"type\":\"message\""));
    assert!(events.contains("\"status\":\"ok\""));
    assert_eq!(latest, "hello back\n");
    assert_eq!(state, "done\n");

    let _ignored = fs::remove_dir_all(&root);
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
    let state = fs::read_to_string(session.join("state"));
    let state = ok!(state);

    assert!(inspect_event_stream_jsonl(&events).is_ok());
    assert!(events.contains("\"type\":\"error\""));
    assert!(events.contains("\"tool\":\"fs.read\""));
    assert!(events.contains("\"code\":\"EACCES\""));
    assert!(events.contains("\"status\":\"error\""));
    assert_eq!(state, "error\n");

    let _ignored = fs::remove_dir_all(&root);
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

    let _ignored = fs::remove_dir_all(&root);
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

    let _ignored = fs::remove_dir_all(&root);
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

    let _ignored = fs::remove_dir_all(&root);
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

    let _ignored = fs::remove_dir_all(&root);
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
    let list = fs::read_to_string(session_root.join("index").join("list"));
    let list = ok!(list);
    let current = fs::read_to_string(session_root.join("index").join("current"));
    let current = ok!(current);
    let by_cwd = fs::read_to_string(session_root.join("index").join("by-cwd").join(by_cwd_key));
    let by_cwd = ok!(by_cwd);

    assert!(messages.contains("\"role\":\"user\""));
    assert!(events.contains("\"type\":\"start\""));
    assert_eq!(list, "default\nreview-1\n");
    assert_eq!(current, "default\n");
    assert_eq!(by_cwd, "default\n");

    let _ignored = fs::remove_dir_all(&root);
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

    let _ignored = fs::remove_dir_all(&root);
}
