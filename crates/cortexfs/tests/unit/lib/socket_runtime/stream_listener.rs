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
