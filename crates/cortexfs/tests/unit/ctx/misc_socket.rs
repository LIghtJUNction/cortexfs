#[test]
fn ctx_latest_run_id_refuses_symlink_events() {
    let root = clean_test_dir("ctx-latest-run-id-symlink-events");
    let outside = clean_test_dir("ctx-latest-run-id-symlink-events-outside");
    let session = root
        .join("home")
        .join("1000")
        .join("agent")
        .join("coder")
        .join("session")
        .join("default");
    create_complete_session_layout(&session);
    write_text_file(
        &outside.join("events.jsonl"),
        "{\"type\":\"start\",\"run\":\"outside\"}\n",
    );
    assert!(fs::remove_file(session.join("events.jsonl")).is_ok());
    assert!(
        std::os::unix::fs::symlink(outside.join("events.jsonl"), session.join("events.jsonl"))
            .is_ok()
    );

    assert!(latest_run_id(&root, "coder", "default").is_err());
    assert_eq!(
        fs::read_to_string(outside.join("events.jsonl")).unwrap_or_default(),
        "{\"type\":\"start\",\"run\":\"outside\"}\n"
    );
}

#[test]
fn ctx_latest_run_id_reads_projected_columnar_events() {
    let root = clean_test_dir("ctx-latest-run-columnar");
    let session = root
        .join("home")
        .join("1000")
        .join("agent/coder/session/default");
    write_text_file(&session.join("messages.jsonl"), "");
    write_text_file(&session.join("events.jsonl"), "");
    assert!(
        columnar::append(
            &session,
            columnar::Stream::Events,
            &[
                r#"{"type":"start","run":"older"}"#,
                r#"{"type":"done","run":"latest","status":"ok"}"#,
            ],
        )
        .is_ok()
    );
    assert_eq!(
        fs::read_to_string(session.join("events.jsonl")).unwrap_or_default(),
        ""
    );

    assert_eq!(
        latest_run_id(&root, "coder", "default"),
        Ok("latest".to_owned())
    );
}

#[test]
fn agent_terminal_runtime_dir_refuses_symlink_parent() {
    let root = clean_test_dir("ctx-agent-terminal-runtime-dir-symlink");
    let outside = clean_test_dir("ctx-agent-terminal-runtime-dir-outside");
    assert!(fs::create_dir_all(root.join("cortexfs")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("cortexfs").join("terminal")).is_ok());

    let path = root
        .join("cortexfs")
        .join("terminal")
        .join("coder")
        .join("default");

    assert!(create_agent_terminal_runtime_dir(&path).is_err());
    assert!(!outside.join("coder").exists());
    assert!(
        root.join("cortexfs")
            .join("terminal")
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    );
}

#[test]
fn ensure_agent_terminal_socket_rejects_symlink_runtime_parent() {
    let root = clean_test_dir("ctx-agent-terminal-socket-runtime-symlink");
    let outside = clean_test_dir("ctx-agent-terminal-socket-runtime-symlink-outside");
    assert!(fs::create_dir_all(root.join("runtime").join("cortexfs")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(
        std::os::unix::fs::symlink(
            &outside,
            root.join("runtime").join("cortexfs").join("terminal")
        )
        .is_ok()
    );
    let visible_socket = root.join("visible").join("main.sock");
    let runtime_socket = root
        .join("runtime")
        .join("cortexfs")
        .join("terminal")
        .join("coder")
        .join("default")
        .join("main.sock");

    assert!(ensure_agent_terminal_socket(&visible_socket, &runtime_socket).is_err());
    assert!(!outside.join("coder").exists());
    assert!(!visible_socket.exists());
}

#[test]
fn ensure_agent_terminal_socket_rejects_symlink_runtime_parent_with_existing_target_dirs() {
    let root = clean_test_dir("ctx-agent-terminal-socket-runtime-existing-symlink");
    let outside = clean_test_dir("ctx-agent-terminal-socket-runtime-existing-symlink-outside");
    assert!(fs::create_dir_all(root.join("runtime").join("cortexfs")).is_ok());
    assert!(fs::create_dir_all(outside.join("coder").join("default")).is_ok());
    assert!(
        std::os::unix::fs::symlink(
            &outside,
            root.join("runtime").join("cortexfs").join("terminal")
        )
        .is_ok()
    );
    let visible_socket = root.join("visible").join("main.sock");
    let runtime_socket = root
        .join("runtime")
        .join("cortexfs")
        .join("terminal")
        .join("coder")
        .join("default")
        .join("main.sock");

    assert!(ensure_agent_terminal_socket(&visible_socket, &runtime_socket).is_err());
    assert!(
        !outside
            .join("coder")
            .join("default")
            .join(".empty-shell-startup")
            .exists()
    );
    assert!(!visible_socket.exists());
}

#[test]
fn ensure_agent_terminal_socket_rejects_symlink_visible_parent_without_writing_target() {
    let root = clean_test_dir("ctx-agent-terminal-socket-visible-symlink");
    let outside = clean_test_dir("ctx-agent-terminal-socket-visible-symlink-outside");
    assert!(fs::create_dir_all(root.join("runtime")).is_ok());
    assert!(fs::create_dir_all(root.join("visible")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("visible").join("terminal")).is_ok());
    let visible_socket = root.join("visible").join("terminal").join("main.sock");
    let runtime_socket = root.join("runtime").join("main.sock");

    assert!(ensure_agent_terminal_socket(&visible_socket, &runtime_socket).is_err());
    assert!(!outside.join("main.sock").exists());
    assert!(
        root.join("visible")
            .join("terminal")
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.file_type().is_symlink())
    );
}

#[test]
fn remove_stale_socket_refuses_plain_file() {
    let root = clean_test_dir("ctx-agent-terminal-remove-plain-file");
    assert!(fs::create_dir_all(&root).is_ok());
    let socket = root.join("main.sock");
    write_text_file(&socket, "keep\n");

    assert!(remove_stale_socket(&socket).is_err());
    assert_eq!(fs::read_to_string(&socket).unwrap_or_default(), "keep\n");
}

#[test]
fn remove_stale_socket_rejects_symlink_parent_without_removing_target_socket()
-> Result<(), Box<dyn std::error::Error>> {
    let root = clean_test_dir("ctx-agent-terminal-remove-parent-symlink");
    let outside = clean_test_dir("ctx-agent-terminal-remove-parent-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    let outside_socket = outside.join("main.sock");
    let listener = std::os::unix::net::UnixListener::bind(&outside_socket)?;
    assert!(std::os::unix::fs::symlink(&outside, root.join("runtime")).is_ok());

    let Err(error) = remove_stale_socket(&root.join("runtime").join("main.sock")) else {
        return Err("symlink parent must fail".into());
    };

    assert!(matches!(
        error.kind(),
        std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::NotADirectory
    ));
    assert!(outside_socket.exists());
    drop(listener);
    Ok(())
}

#[test]
fn terminal_socket_exists_rejects_plain_file() {
    let root = clean_test_dir("ctx-terminal-socket-exists-plain-file");
    let socket = root.join("main.sock");
    write_text_file(&socket, "not a socket\n");

    assert!(!terminal_socket_exists(&socket));
}

#[test]
fn agent_chat_request_socket_prefers_runtime_socket_over_visible_socket()
-> Result<(), Box<dyn std::error::Error>> {
    const ISOLATED: &str = "CORTEXFS_TEST_AGENT_CHAT_RUNTIME_SOCKET";
    if std::env::var_os(ISOLATED).is_none() {
        let runtime = tempfile::Builder::new().prefix("cfs-rt-").tempdir()?;
        let status = std::process::Command::new(std::env::current_exe()?)
            .arg("tests::agent_chat_request_socket_prefers_runtime_socket_over_visible_socket")
            .arg("--exact")
            .env(ISOLATED, "1")
            .env("XDG_RUNTIME_DIR", runtime.path())
            .status()?;
        if !status.success() {
            return Err("isolated runtime socket preference test failed".into());
        }
        return Ok(());
    }

    let root = clean_test_dir("ctx-agent-chat-request-prefers-runtime");
    let visible_parent = root.join("agent");
    fs::create_dir_all(&visible_parent)?;
    let visible_socket = visible_parent.join("coder.sock");
    let visible_listener = std::os::unix::net::UnixListener::bind(&visible_socket)?;
    let runtime_socket = match agent_chat_runtime_socket(&root, "coder") {
        Ok(socket) => socket,
        Err(error) => {
            return Err(format!("runtime chat socket path should be available: {error:?}").into());
        }
    };
    let runtime_parent = runtime_socket
        .parent()
        .ok_or("runtime socket has no parent")?;
    fs::create_dir_all(runtime_parent)?;
    let runtime_listener = std::os::unix::net::UnixListener::bind(&runtime_socket)?;

    let selected_socket = match agent_chat_request_socket(&root, "coder") {
        Ok(socket) => socket,
        Err(error) => {
            return Err(format!("chat request socket should be available: {error:?}").into());
        }
    };
    assert_eq!(selected_socket, runtime_socket);

    drop(runtime_listener);
    drop(visible_listener);
    let _ignored = fs::remove_file(runtime_socket);
    Ok(())
}

#[test]
fn socket_bind_path_rejects_symlink_parent() {
    let root = clean_test_dir("ctx-terminal-socket-bind-parent-symlink");
    let outside = clean_test_dir("ctx-terminal-socket-bind-parent-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("runtime")).is_ok());
    assert!(
        std::os::unix::fs::symlink(
            outside.join("runtime").join("main.sock"),
            outside.join("main.sock")
        )
        .is_ok()
    );
    assert!(std::os::unix::fs::symlink(&outside, root.join("visible")).is_ok());

    let socket = root.join("visible").join("main.sock");

    assert_eq!(socket_bind_path(&socket), socket);
}

#[test]
fn oauth_callback_reader_stops_after_headers() {
    let request = b"GET /callback?code=ok HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\nignored body";

    let read = read_oauth_callback_request_from_reader(
        std::io::Cursor::new(request.as_slice()),
        MAX_OAUTH_CALLBACK_REQUEST_BYTES,
    );

    assert_eq!(
        read.unwrap_or_default(),
        "GET /callback?code=ok HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
    );
}

#[test]
fn oauth_callback_reader_rejects_oversized_headers() {
    let request = vec![b'a'; MAX_OAUTH_CALLBACK_REQUEST_BYTES + 1];

    let read = read_oauth_callback_request_from_reader(
        std::io::Cursor::new(request),
        MAX_OAUTH_CALLBACK_REQUEST_BYTES,
    );

    assert!(matches!(read, Err(ref error) if error.code == 69));
}

#[test]
fn oauth_callback_reader_maps_idle_timeout_to_callback_timeout() {
    struct TimeoutReader;

    impl std::io::Read for TimeoutReader {
        fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "idle callback",
            ))
        }
    }

    let read =
        read_oauth_callback_request_from_reader(TimeoutReader, MAX_OAUTH_CALLBACK_REQUEST_BYTES);

    assert!(matches!(
        read,
        Err(ref error) if error.code == 69 && error.message == "oauth callback timed out"
    ));
}

#[test]
fn oauth_callback_parser_accepts_http_1_request_line() {
    let parsed = parse_oauth_callback_params(
        "GET /callback?code=ok&state=s HTTP/1.1\r\n\r\n",
        "/callback",
    );

    assert!(matches!(
        parsed,
        Ok(ref params)
            if params.code.as_deref() == Some("ok")
                && params.state.as_deref() == Some("s")
    ));
}

#[test]
fn oauth_callback_parser_rejects_invalid_request_lines_and_parameters() {
    for request in [
        "GET /callback?code=ok&state=s\n\n",
        "GET /callback?code=ok&state=s HTTP/1.1 extra\n\n",
        "GET /callback?code=one&code=two HTTP/1.1\r\n\r\n",
        "GET /callback?code=ok&state=one&state=two HTTP/1.1\r\n\r\n",
        "GET /callback?code=&state=s HTTP/1.1\r\n\r\n",
        "GET /callback?code=ok&state= HTTP/1.1\r\n\r\n",
    ] {
        assert!(
            matches!(parse_oauth_callback_params(request, "/callback"), Err(ref error) if error.code == 2)
        );
    }
}

#[test]
fn device_code_handles_slow_down_pending_and_exchange() -> Result<(), cortexfs::OAuthError> {
    let device = cortexfs::request_device_code_with(|url, body| {
        assert_eq!(url, cortexfs::CODEX_DEVICE_USER_URL);
        assert!(body.contains(cortexfs::CODEX_CLIENT_ID));
        Ok((
            200,
            br#"{"device_auth_id":"id","user_code":"ABCD","interval":"1"}"#.to_vec(),
        ))
    });
    let device = device?;
    assert_eq!(
        (device.code.as_str(), device.interval.as_str()),
        ("ABCD", "1")
    );
    let calls = Cell::new(0);
    let mut waits = Vec::new();
    let token = cortexfs::poll_device_code_with(
        &device,
        20,
        |_url, _body| {
            let call = calls.get();
            calls.set(call + 1);
            Ok(match call {
            0 => (429, br#"{"error":"slow_down"}"#.to_vec()),
            1 => (403, br#"{"error":"authorization_pending"}"#.to_vec()),
            _ => (200, br#"{"authorization_code":"auth","code_verifier":"dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk","code_challenge":"E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"}"#.to_vec()),
        })
        },
        |url, form| {
            assert_eq!(url, "https://auth.openai.com/oauth/token");
            assert!(
                form.contains("redirect_uri=https%3A%2F%2Fauth.openai.com%2Fdeviceauth%2Fcallback")
            );
            Ok((
                200,
                br#"{"access_token":"access","refresh_token":"refresh"}"#.to_vec(),
            ))
        },
        |seconds| waits.push(seconds),
    );
    assert_eq!(
        token.map(|value| value.access_token),
        Ok("access".to_owned())
    );
    assert_eq!(waits, [6, 6]);
    Ok(())
}

#[test]
fn current_session_name_falls_back_to_default_when_index_is_unreadable() {
    let root = clean_test_dir("ctx-current-session-unreadable");
    let index = root.join("index");
    assert!(
        fs::create_dir_all(&index).is_ok(),
        "failed to create session index directory"
    );
    let current = index.join("current");
    assert!(
        fs::write(&current, "custom\n").is_ok(),
        "failed to write current session override"
    );
    assert!(
        fs::set_permissions(&current, fs::Permissions::from_mode(0o000)).is_ok(),
        "failed to set current session file unreadable"
    );

    let result = current_session_name(&root);

    assert!(
        fs::set_permissions(&current, fs::Permissions::from_mode(0o600)).is_ok(),
        "failed to restore current session file permissions"
    );
    let Ok(name) = result else {
        return;
    };
    assert_eq!(name, "default");
}

#[test]
fn current_session_name_falls_back_to_default_when_index_points_to_deleted_session() {
    let root = clean_test_dir("ctx-current-session-deleted");
    let index = root.join("index");
    assert!(fs::create_dir_all(&index).is_ok());
    assert!(fs::write(index.join("current"), "deleted-session\n").is_ok());

    let name = current_session_name(&root);

    assert!(matches!(name, Ok(ref name) if name == "default"));
}

#[test]
fn current_session_name_rejects_symlink_index_dir_without_reading_target() {
    let root = clean_test_dir("ctx-current-session-symlink-index");
    let outside = clean_test_dir("ctx-current-session-symlink-index-target");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::write(outside.join("current"), "outside\n").is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("index")).is_ok());

    let result = current_session_name(&root);

    assert!(result.is_err());
}

#[test]
fn current_session_name_rejects_symlink_current_file_without_reading_target() {
    let root = clean_test_dir("ctx-current-session-symlink-current");
    let outside = clean_test_dir("ctx-current-session-symlink-current-target");
    let index = root.join("index");
    assert!(fs::create_dir_all(&index).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::write(outside.join("current"), "outside\n").is_ok());
    assert!(std::os::unix::fs::symlink(outside.join("current"), index.join("current")).is_ok());

    let result = current_session_name(&root);

    assert!(result.is_err());
}
