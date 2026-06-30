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
