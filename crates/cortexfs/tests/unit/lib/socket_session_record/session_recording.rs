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
    assert!(recorded.messages().first().is_some_and(|message| {
        serde_json::from_str::<serde_json::Value>(message)
            .ok()
            .and_then(|value| value.get("run").and_then(serde_json::Value::as_str).map(str::to_owned))
            .as_deref()
            == Some("run-1")
    }));
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
