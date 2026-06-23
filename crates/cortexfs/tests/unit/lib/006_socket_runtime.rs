#[test]
fn indexed_socket_send_rejects_temp_sessions_before_index_update() {
    let root = unique_test_dir("indexed-socket-temp");
    let session_root = root.join("session");
    let session = session_root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    create_complete_session_layout(&session);
    write_text_file(&session_root.join("index").join("list"), "default\n");
    write_text_file(&session_root.join("index").join("current"), "default\n");
    assert!(fs::create_dir_all(session_root.join("index").join("by-cwd")).is_ok());

    let temp = parse_socket_request_frame(
        r#"{"op":"send","id":"msg-1","session":"default","scope":"temp","cwd":"/work","input":"hello"}"#,
    );
    assert!(temp.is_ok());
    let Ok(temp) = temp else { return };
    assert_eq!(
        record_indexed_socket_send_to_session(&session_root, &temp),
        Err(IndexedSocketSessionRecordError::Session(
            SocketSessionRecordError::TempSessionNotDurable
        ))
    );
    let list = fs::read_to_string(session_root.join("index").join("list"));
    assert!(list.is_ok());
    let Ok(list) = list else { return };
    assert_eq!(list, "default\n");
    assert!(!session_root
        .join("index")
        .join("by-cwd")
        .join("cwd")
        .exists());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn durable_session_layout_helper_creates_inspectable_session_and_index() {
    let root = unique_test_dir("durable-session-layout");
    let session_root = root.join("session");
    let session = session_root.join("default");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

    let ensured = ensure_durable_session_layout(
        &session_root,
        "default",
        "/work/project",
        Some("debug/echo"),
        SocketSessionScope::Private,
    );
    assert_eq!(ensured, Ok(()));
    assert!(inspect_session_layout(&session).is_ok());

    let list = fs::read_to_string(session_root.join("index").join("list"));
    assert!(list.is_ok());
    let Ok(list) = list else { return };
    let current = fs::read_to_string(session_root.join("index").join("current"));
    assert!(current.is_ok());
    let Ok(current) = current else { return };
    let meta = fs::read_to_string(session.join("meta.json"));
    assert!(meta.is_ok());
    let Ok(meta) = meta else { return };
    let pack = fs::read_to_string(session.join("context").join("pack.json"));
    assert!(pack.is_ok());
    let Ok(pack) = pack else { return };

    assert_eq!(list, "default\n");
    assert_eq!(current, "default\n");
    assert!(meta.contains("\"model\":\"debug/echo\""));
    assert!(meta.contains("\"scope\":\"private\""));
    assert!(inspect_context_pack_json(&pack).is_ok());

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
    assert!(request.is_ok());
    let Ok(request) = request else { return };
    assert!(record_indexed_socket_send_to_session(&session_root, &request).is_ok());
    let state = fs::read_to_string(session.join("state"));
    assert!(state.is_ok());
    let Ok(state) = state else { return };
    assert_eq!(state, "active\n");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn durable_session_layout_helper_rejects_invalid_durable_inputs() {
    let root = unique_test_dir("durable-session-layout-invalid");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

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

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn socket_runtime_handles_ping_send_resume_and_cancel() {
    let root = unique_test_dir("socket-runtime");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

    let ping =
        handle_socket_request_frame(&session_root, "/work", Some("debug/echo"), r#"{"op":"ping"}"#);
    assert!(ping.is_ok());
    let Ok(ping) = ping else { return };
    assert_eq!(ping.jsonl(), "{\"type\":\"pong\"}\n");

    let send = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"send","id":"msg-1","session":"default","input":"hello"}"#,
    );
    assert!(send.is_ok());
    let Ok(send) = send else { return };
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
    assert!(resume_all.is_ok());
    let Ok(resume_all) = resume_all else { return };
    assert_eq!(resume_all.frames().len(), 2);
    assert!(resume_all.jsonl().contains("\"run\":\"msg-1\""));
    assert!(resume_all.jsonl().contains("\"run\":\"msg-2\""));

    let resume_after = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"resume","session":"default","after":"msg-1"}"#,
    );
    assert!(resume_after.is_ok());
    let Ok(resume_after) = resume_after else {
        return;
    };
    assert_eq!(resume_after.frames().len(), 1);
    assert!(!resume_after.jsonl().contains("\"run\":\"msg-1\""));
    assert!(resume_after.jsonl().contains("\"run\":\"msg-2\""));

    let cancel = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"cancel","id":"msg-2"}"#,
    );
    assert!(cancel.is_ok());
    let Ok(cancel) = cancel else { return };
    assert!(cancel.jsonl().contains("\"status\":\"cancelled\""));
    let state = fs::read_to_string(session_root.join("default").join("state"));
    assert!(state.is_ok());
    let Ok(state) = state else { return };
    assert_eq!(state, "cancelled\n");

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn socket_runtime_temp_send_does_not_create_durable_session() {
    let root = unique_test_dir("socket-runtime-temp");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

    let send = handle_socket_request_frame(
        &session_root,
        "/work",
        Some("debug/echo"),
        r#"{"op":"send","id":"msg-1","session":"scratch","scope":"temp","input":"hello"}"#,
    );
    assert!(send.is_ok());
    let Ok(send) = send else { return };
    assert_eq!(send.frames().len(), 1);
    assert!(send.jsonl().contains("\"type\":\"start\""));
    assert!(send.jsonl().contains("\"model\":\"debug/echo\""));
    assert!(!session_root.exists());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn socket_runtime_errors_convert_to_stable_error_frames() {
    let root = unique_test_dir("socket-runtime-error");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());

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
    assert!(parsed.is_ok());
    let Ok(parsed) = parsed else { return };
    assert_eq!(
        parsed.get("type").and_then(serde_json::Value::as_str),
        Some("error")
    );
    assert_eq!(
        parsed.get("code").and_then(serde_json::Value::as_str),
        Some("EINVAL")
    );

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn socket_stream_runtime_serves_one_frame_with_peer_credentials() {
    let root = unique_test_dir("socket-stream-runtime");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    let pair = UnixStream::pair();
    assert!(pair.is_ok());
    let Ok((mut client, mut socket)) = pair else {
        return;
    };
    let peer = peer_credentials(&socket);
    assert!(peer.is_ok());
    let Ok(peer) = peer else { return };
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
    assert!(outcome.is_ok());
    let Ok(outcome) = outcome else { return };
    assert_eq!(outcome.frames().len(), 1);

    let mut buffer = [0_u8; 256];
    let read = client.read(&mut buffer);
    assert!(read.is_ok());
    let Ok(read) = read else { return };
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"type\":\"start\""));
    assert!(response.contains("\"run\":\"msg-1\""));
    assert!(inspect_session_layout(&session_root.join("default")).is_ok());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn socket_stream_runtime_denies_wrong_peer_before_mutating_session() {
    let root = unique_test_dir("socket-stream-runtime-deny");
    let session_root = root.join("session");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    let pair = UnixStream::pair();
    assert!(pair.is_ok());
    let Ok((mut client, mut socket)) = pair else {
        return;
    };
    let peer = peer_credentials(&socket);
    assert!(peer.is_ok());
    let Ok(peer) = peer else { return };
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
    assert!(read.is_ok());
    let Ok(read) = read else { return };
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"type\":\"error\""));
    assert!(response.contains("\"code\":\"EACCES\""));
    assert!(!session_root.exists());

    let _ignored = fs::remove_dir_all(&root);
}

#[test]
fn socket_listener_runtime_accepts_and_serves_one_connection() {
    let root = unique_test_dir("socket-listener-runtime");
    let session_root = root.join("session");
    let socket_path = root.join("agent.sock");
    assert!(fs::remove_dir_all(&root).is_ok() || !root.exists());
    assert!(fs::create_dir_all(&root).is_ok());
    let listener = UnixListener::bind(&socket_path);
    assert!(listener.is_ok());
    let Ok(listener) = listener else { return };

    let client = UnixStream::connect(&socket_path);
    assert!(client.is_ok());
    let Ok(mut client) = client else { return };
    assert!(client
        .write_all(
            br#"{"op":"send","id":"msg-1","session":"default","input":"hello"}
"#,
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome =
        serve_unix_socket_listener_once(&listener, None, &session_root, "/work", Some("debug/echo"));
    assert!(outcome.is_ok());
    let Ok(outcome) = outcome else { return };
    assert_eq!(outcome.frames().len(), 1);

    let mut buffer = [0_u8; 256];
    let read = client.read(&mut buffer);
    assert!(read.is_ok());
    let Ok(read) = read else { return };
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"type\":\"start\""));
    assert!(response.contains("\"run\":\"msg-1\""));
    assert!(inspect_session_layout(&session_root.join("default")).is_ok());

    let _ignored = fs::remove_dir_all(&root);
}
