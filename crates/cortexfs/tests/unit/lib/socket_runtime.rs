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
    assert!(!session_root
        .join("index")
        .join("by-cwd")
        .join("cwd")
        .exists());
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
    assert_eq!(ensured, Ok(()));
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
        fs::metadata(session.join("messages.jsonl"))
            .map(|meta| meta.permissions().mode() & 0o777),
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
    assert_eq!(updated, Ok(()));
    let meta = fs::read_to_string(session.join("meta.json"));
    assert!(matches!(meta, Ok(ref meta) if meta.contains("\"model\":\"openai/gpt-4o\"")));

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

#[test]
fn socket_stream_runtime_serves_one_frame_with_peer_credentials() {
    let root = clean_test_dir("socket-stream-runtime");
    let session_root = root.join("session");

    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);
    let peer = peer_credentials(&socket);
    let peer = ok!(peer);
    let policy = SocketPeerPolicy::uid_gid(peer.uid(), peer.gid());

    assert!(client
        .write_all(
            br#"{"op":"send","id":"msg-1","session":"default","input":"hello"}
"#,
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_unix_socket_stream_once(
        &mut socket,
        Some(policy),
        &session_root,
        "/work",
        Some("debug/echo"),
    );
    let outcome = ok!(outcome);
    assert_eq!(outcome.frames().len(), 1);

    let mut buffer = [0_u8; 256];
    let read = client.read(&mut buffer);
    let read = ok!(read);
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"type\":\"start\""));
    assert!(response.contains("\"run\":\"msg-1\""));
    assert!(inspect_session_layout(&session_root.join("default")).is_ok());
}

#[test]
fn agent_socket_runtime_answers_missing_file_path_without_model() {
    let root = clean_test_dir("agent-socket-runtime-local-read-path");
    let session_root = root.join("session");
    let identity = AgentUnixIdentity::new(1000, 1000, []);
    let executable = root.join("missing-agent-executable");
    let runtime = AgentExecutableSocketRuntime {
        ctx_root: &root,
        source_root: &root,
        identity: &identity,
        env: &[],
        session_root: &session_root,
        default_cwd: "/work",
        model: Some("debug/echo"),
        agent_name: "coder",
        agent_executable: &executable,
    };

    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);
    assert!(
        client
            .write_all(
                "{\"op\":\"send\",\"id\":\"msg-1\",\"session\":\"default\",\"input\":\"可以，尝试读文件\"}\n"
                    .as_bytes(),
            )
            .is_ok()
    );
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(&mut socket, None, runtime);
    let outcome = ok!(outcome);
    assert_eq!(outcome.frames().len(), 3);
    assert!(outcome.jsonl().contains("\"type\":\"start\""));
    assert!(outcome.jsonl().contains("请提供要读取的文件路径。"));
    assert!(outcome.jsonl().contains("\"status\":\"ok\""));
    assert!(fs::read_to_string(session_root.join("default").join("events.jsonl"))
        .unwrap_or_default()
        .contains("请提供要读取的文件路径。"));

    let mut buffer = [0_u8; 1024];
    let read = client.read(&mut buffer);
    let read = ok!(read);
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"type\":\"start\""));
    assert!(response.contains("请提供要读取的文件路径。"));
    assert!(response.contains("\"status\":\"ok\""));
}

#[test]
fn socket_stream_runtime_denies_wrong_peer_before_mutating_session() {
    let root = clean_test_dir("socket-stream-runtime-deny");
    let session_root = root.join("session");

    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);
    let peer = peer_credentials(&socket);
    let peer = ok!(peer);
    let denied_uid = if peer.uid() == u32::MAX {
        peer.uid() - 1
    } else {
        peer.uid() + 1
    };
    let policy = SocketPeerPolicy::uid(denied_uid);

    assert!(client
        .write_all(
            br#"{"op":"send","id":"msg-1","session":"default","input":"hello"}
"#,
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_unix_socket_stream_once(
        &mut socket,
        Some(policy),
        &session_root,
        "/work",
        Some("debug/echo"),
    );
    assert_eq!(outcome, Err(SocketRuntimeError::PeerDenied));

    let mut buffer = [0_u8; 256];
    let read = client.read(&mut buffer);
    let read = ok!(read);
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"type\":\"error\""));
    assert!(response.contains("\"code\":\"EACCES\""));
    assert!(!session_root.exists());
}

#[test]
fn socket_stream_runtime_times_out_idle_client_before_mutating_session() {
    let root = clean_test_dir("socket-stream-runtime-idle-timeout");
    let session_root = root.join("session");

    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);
    assert!(socket
        .set_read_timeout(Some(Duration::from_millis(1)))
        .is_ok());

    let outcome = serve_unix_socket_stream_once(&mut socket, None, &session_root, "/work", None);

    assert_eq!(outcome, Err(SocketRuntimeError::CannotReadFrame));
    let mut buffer = [0_u8; 256];
    let read = client.read(&mut buffer);
    let read = ok!(read);
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"type\":\"error\""));
    assert!(response.contains("\"code\":\"EIO\""));
    assert!(!session_root.exists());
}

#[test]
fn socket_stream_runtime_restores_default_read_timeout_after_read_error() {
    let root = clean_test_dir("socket-stream-runtime-timeout-restore");
    let session_root = root.join("session");

    let pair = UnixStream::pair();
    let (client, mut socket) = ok!(pair);
    assert_eq!(socket.read_timeout().ok().flatten(), None);
    drop(client);

    let outcome = serve_unix_socket_stream_once(&mut socket, None, &session_root, "/work", None);

    assert!(outcome.is_err());
    assert_eq!(socket.read_timeout().ok().flatten(), None);
}

#[test]
fn socket_listener_runtime_accepts_and_serves_one_connection() {
    let root = clean_test_dir("socket-listener-runtime");
    let session_root = root.join("session");
    let socket_path = root.join("agent.sock");
    assert!(fs::create_dir_all(&root).is_ok());
    let listener = UnixListener::bind(&socket_path);
    let listener = ok!(listener);

    let client = UnixStream::connect(&socket_path);
    let mut client = ok!(client);
    assert!(client
        .write_all(
            br#"{"op":"send","id":"msg-1","session":"default","input":"hello"}
"#,
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome =
        serve_unix_socket_listener_once(&listener, None, &session_root, "/work", Some("debug/echo"));
    let outcome = ok!(outcome);
    assert_eq!(outcome.frames().len(), 1);

    let mut buffer = [0_u8; 256];
    let read = client.read(&mut buffer);
    let read = ok!(read);
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"type\":\"start\""));
    assert!(response.contains("\"run\":\"msg-1\""));
    assert!(inspect_session_layout(&session_root.join("default")).is_ok());
}
