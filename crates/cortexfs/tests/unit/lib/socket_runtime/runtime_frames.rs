#[test]
fn socket_runtime_handles_ping_send_resume_and_cancel() {
    let root = clean_test_dir("socket-runtime");
    let session_root = root.join("session");


    let ping =
        handle_socket_request_frame(&session_root, "/work", Some("debug/echo"), r#"{"op":"ping"}"#);
    let ping = ok!(ping);
    assert_eq!(ping.jsonl(), "{\"type\":\"pong\"}\n");

    let send = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"send","id":"msg-1","session":"default","input":"hello"}"#,
    );
    let send = ok!(send);
    assert_eq!(send.frames().len(), 1);
    assert!(send.jsonl().contains("\"type\":\"start\""));
    assert!(send.jsonl().contains("\"run\":\"msg-1\""));
    assert!(inspect_session_layout(&session_root.join("default")).is_ok());

    let second = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"send","id":"msg-2","session":"default","input":"again"}"#,
    );
    assert!(second.is_ok());

    let resume_all = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"resume","session":"default"}"#,
    );
    let resume_all = ok!(resume_all);
    assert_eq!(resume_all.frames().len(), 2);
    assert!(resume_all.jsonl().contains("\"run\":\"msg-1\""));
    assert!(resume_all.jsonl().contains("\"run\":\"msg-2\""));

    let resume_after = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"resume","session":"default","after":"msg-1"}"#,
    );
    let resume_after = ok!(resume_after);
    assert_eq!(resume_after.frames().len(), 1);
    assert!(!resume_after.jsonl().contains("\"run\":\"msg-1\""));
    assert!(resume_after.jsonl().contains("\"run\":\"msg-2\""));

    let cancel = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"cancel","id":"msg-2"}"#,
    );
    let cancel = ok!(cancel);
    assert!(cancel.jsonl().contains("\"status\":\"cancelled\""));
    assert_file_text(&session_root.join("default").join("state"), "cancelled\n");
}

#[test]
fn socket_runtime_resume_rejects_symlink_and_oversized_events() {
    let root = clean_test_dir("socket-runtime-resume-events-hardening");
    let session_root = root.join("session");
    let session = session_root.join("default");
    let outside = clean_test_dir("socket-runtime-resume-events-hardening-outside");
    create_complete_session_layout(&session);
    write_text_file(&outside.join("events.jsonl"), "{\"type\":\"start\",\"run\":\"outside\"}\n");
    assert!(fs::remove_file(session.join("events.jsonl")).is_ok());
    assert!(symlink(outside.join("events.jsonl"), session.join("events.jsonl")).is_ok());

    assert_eq!(
        handle_socket_request_frame(
            &session_root,
            "/work",
            Some("debug/echo"),
            r#"{"op":"resume","session":"default"}"#
        ),
        Err(SocketRuntimeError::CannotReadEvents)
    );

    assert!(fs::remove_file(session.join("events.jsonl")).is_ok());
    write_text_file(&session.join("events.jsonl"), &"x".repeat((1024 * 1024) + 1));
    assert_eq!(
        handle_socket_request_frame(
            &session_root,
            "/work",
            Some("debug/echo"),
            r#"{"op":"resume","session":"default"}"#
        ),
        Err(SocketRuntimeError::CannotReadEvents)
    );
}

#[test]
fn socket_runtime_resume_rejects_symlink_intermediate_session_dir() {
    let root = clean_test_dir("socket-runtime-resume-events-symlink-intermediate");
    let session_root = root.join("session");
    let outside = clean_test_dir("socket-runtime-resume-events-symlink-intermediate-outside");
    create_complete_session_layout(&outside.join("default"));
    write_text_file(
        &outside.join("default").join("events.jsonl"),
        "{\"type\":\"start\",\"run\":\"outside\"}\n",
    );
    assert!(fs::create_dir_all(&session_root).is_ok());
    assert!(symlink(&outside, session_root.join("default")).is_ok());

    assert_eq!(
        handle_socket_request_frame(
            &session_root,
            "/work",
            Some("debug/echo"),
            r#"{"op":"resume","session":"default"}"#
        ),
        Err(SocketRuntimeError::CannotReadEvents)
    );
    assert_file_text(
        &outside.join("default").join("events.jsonl"),
        "{\"type\":\"start\",\"run\":\"outside\"}\n",
    );
}

#[test]
fn socket_runtime_cancel_rejects_symlink_current_index() {
    let root = clean_test_dir("socket-runtime-current-index-symlink");
    let session_root = root.join("session");
    let session = session_root.join("default");
    let outside = clean_test_dir("socket-runtime-current-index-symlink-outside");
    create_complete_session_layout(&session);
    assert!(fs::create_dir_all(session_root.join("index")).is_ok());
    write_text_file(&outside.join("current"), "default\n");
    assert!(symlink(outside.join("current"), session_root.join("index").join("current")).is_ok());

    assert_eq!(
        handle_socket_request_frame(
            &session_root,
            "/work",
            Some("debug/echo"),
            r#"{"op":"cancel","id":"run-1"}"#
        ),
        Err(SocketRuntimeError::CannotReadEvents)
    );
    assert_file_text(&outside.join("current"), "default\n");
}

#[test]
fn socket_runtime_temp_send_does_not_create_durable_session() {
    let root = clean_test_dir("socket-runtime-temp");
    let session_root = root.join("session");


    let send = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"send","id":"msg-1","session":"scratch","scope":"temp","input":"hello"}"#,
    );
    let send = ok!(send);
    assert_eq!(send.frames().len(), 1);
    assert!(send.jsonl().contains("\"type\":\"start\""));
    assert!(send.jsonl().contains("\"model\":\"debug/echo\""));
    assert!(!session_root.exists());
}

#[test]
fn socket_runtime_errors_convert_to_stable_error_frames() {
    let root = clean_test_dir("socket-runtime-error");
    let session_root = root.join("session");


    let error = handle_socket_request_frame(
        &session_root,
        "/work/../bad",
        Some("debug/echo"),
        r#"{"op":"send","id":"msg-1","session":"default","input":"hello"}"#,
    );
    assert_eq!(
        error,
        Err(SocketRuntimeError::SessionLayout(
            DurableSessionLayoutError::InvalidCwd
        ))
    );
    let Err(error) = error else { return };
    let response = socket_runtime_error_response(&error);
    assert_eq!(
        response.jsonl(),
        "{\"code\":\"EINVAL\",\"message\":\"EINVAL\",\"type\":\"error\"}\n"
    );
    let Some(frame) = response.frames().first() else {
        return;
    };
    let parsed = serde_json::from_str::<serde_json::Value>(frame);
    let parsed = ok!(parsed);
    assert_eq!(
        parsed.get("type").and_then(serde_json::Value::as_str),
        Some("error")
    );
    assert_eq!(
        parsed.get("code").and_then(serde_json::Value::as_str),
        Some("EINVAL")
    );
}
