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
    assert_file_text(&session.join("current_run"), "msg-1\n");
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
        workspace: Some("/repo".to_owned()),
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
use super::*;
