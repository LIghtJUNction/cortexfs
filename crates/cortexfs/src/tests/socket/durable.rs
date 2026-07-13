#[test]
fn indexed_socket_send_rejects_temp_sessions_before_index_update() {
    let root = clean_test_dir("indexed-socket-temp");
    let session_root = root.join("session");
    let session = session_root.join("default");

    create_complete_session_layout(&session);
    write_text_file(&session_root.join("index").join("list"), "default\n");
    write_text_file(&session_root.join("index").join("current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());

    let temp = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","scope":"temp","cwd":"/work","input":"hello"}"#,
    );
    let temp = ok!(temp);
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &temp),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::TempSessionNotDurable
        ))
    );
    assert_file_text(&session_root.join("index").join("list"), "default\n");
    assert!(
        !session_root
            .join("index")
            .join("by-cwd")
            .join("cwd")
            .exists()
    );
}

#[test]
fn durable_session_layout_helper_creates_inspectable_session_and_index() {
    let root = clean_test_dir("durable-session-layout");
    let session_root = root.join("session");
    let session = session_root.join("default");

    let ensured = ensure_durable_session_layout(
        &session_root,
        "default",
        "/work/project",
        Some("main"),
        SocketSessionScope::Private,
    );
    assert!(ensured.is_ok());
    assert!(inspect_session_layout(&session).is_ok());

    let meta = fs::read_to_string(session.join("meta.json"));
    let meta = ok!(meta);
    let pack = fs::read_to_string(session.join("context").join("pack.json"));
    let pack = ok!(pack);

    assert_file_text(&session_root.join("index").join("list"), "default\n");
    assert_file_text(&session_root.join("index").join("current"), "default\n");
    assert!(session_root.join("index").join("by-cwd").is_dir());
    assert!(session_root.join("index").join("by-hash").is_dir());
    assert!(session_root.join("index").join("by-uuid").is_dir());
    assert!(meta.contains("\"model\":\"main\""));
    assert!(meta.contains("\"scope\":\"private\""));
    assert!(inspect_context_pack_json(&pack).is_ok());
    assert!(matches!(
        fs::metadata(&session_root).map(|meta| meta.permissions().mode() & 0o777),
        Ok(0o700)
    ));
    assert!(matches!(
        fs::metadata(&session).map(|meta| meta.permissions().mode() & 0o777),
        Ok(0o700)
    ));
    assert!(matches!(
        fs::metadata(session.join("context")).map(|meta| meta.permissions().mode() & 0o777),
        Ok(0o700)
    ));
    assert!(matches!(
        fs::metadata(session.join("messages.jsonl")).map(|meta| meta.permissions().mode() & 0o777),
        Ok(0o600)
    ));
    assert!(matches!(
        fs::metadata(session.join("context").join("summary.md"))
            .map(|meta| meta.permissions().mode() & 0o777),
        Ok(0o600)
    ));

    let updated = ensure_durable_session_layout(
        &session_root,
        "default",
        "/work/project",
        Some("openai/gpt-4o"),
        SocketSessionScope::Private,
    );
    assert!(updated.is_ok());
    let meta = fs::read_to_string(session.join("meta.json"));
    assert!(matches!(meta, Ok(ref meta) if meta.contains("\"model\":\"main\"")));

    let request = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","cwd":"/work/project","input":"hello"}"#,
    );
    let request = ok!(request);
    assert!(record_indexed_socket_send_to_session(&session_root, &request).is_ok());
    assert_file_text(&session.join("state"), "active\n");
}

#[test]
fn durable_session_layout_helper_rejects_invalid_durable_inputs() {
    let root = clean_test_dir("durable-session-layout-invalid");
    let session_root = root.join("session");

    assert_eq!(
        ensure_durable_session_layout(
            &session_root,
            "bad/name",
            "/work",
            None,
            SocketSessionScope::Private,
        ),
        Err(DurableSessionLayoutError::InvalidSessionName)
    );
    assert_eq!(
        ensure_durable_session_layout(
            &session_root,
            "default",
            "../host",
            None,
            SocketSessionScope::Private,
        ),
        Err(DurableSessionLayoutError::InvalidCwd)
    );
    assert_eq!(
        ensure_durable_session_layout(
            &session_root,
            "default",
            "/work",
            Some("bad/model/extra"),
            SocketSessionScope::Private,
        ),
        Err(DurableSessionLayoutError::InvalidModelName)
    );
    assert_eq!(
        ensure_durable_session_layout(
            &session_root,
            "default",
            "/work",
            None,
            SocketSessionScope::Temp,
        ),
        Err(DurableSessionLayoutError::TempSessionNotDurable)
    );
    assert_eq!(DurableSessionLayoutError::InvalidCwd.errno(), "EINVAL");
    assert!(!session_root.exists());
}
use super::*;
