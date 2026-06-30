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
    write_text_file(&outside.join("events.jsonl"), "{\"type\":\"start\",\"run\":\"outside\"}\n");
    assert!(fs::remove_file(session.join("events.jsonl")).is_ok());
    assert!(std::os::unix::fs::symlink(
        outside.join("events.jsonl"),
        session.join("events.jsonl")
    )
    .is_ok());

    assert!(latest_run_id(&root, "coder", "default").is_err());
    assert_eq!(
        fs::read_to_string(outside.join("events.jsonl")).unwrap_or_default(),
        "{\"type\":\"start\",\"run\":\"outside\"}\n"
    );
}

#[test]
fn agent_terminal_runtime_dir_refuses_symlink_parent() {
    let root = clean_test_dir("ctx-agent-terminal-runtime-dir-symlink");
    let outside = clean_test_dir("ctx-agent-terminal-runtime-dir-outside");
    assert!(fs::create_dir_all(root.join("cortexfs")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("cortexfs").join("terminal")).is_ok());

    let path = root.join("cortexfs").join("terminal").join("coder").join("default");

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
    assert!(std::os::unix::fs::symlink(
        &outside,
        root.join("runtime").join("cortexfs").join("terminal")
    )
    .is_ok());
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
    assert!(std::os::unix::fs::symlink(
        &outside,
        root.join("runtime").join("cortexfs").join("terminal")
    )
    .is_ok());
    let visible_socket = root.join("visible").join("main.sock");
    let runtime_socket = root
        .join("runtime")
        .join("cortexfs")
        .join("terminal")
        .join("coder")
        .join("default")
        .join("main.sock");

    assert!(ensure_agent_terminal_socket(&visible_socket, &runtime_socket).is_err());
    assert!(!outside
        .join("coder")
        .join("default")
        .join(".empty-shell-startup")
        .exists());
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
    assert!(root
        .join("visible")
        .join("terminal")
        .symlink_metadata()
        .is_ok_and(|metadata| metadata.file_type().is_symlink()));
}

#[test]
fn remove_stale_agent_terminal_socket_refuses_plain_file() {
    let root = clean_test_dir("ctx-agent-terminal-remove-plain-file");
    assert!(fs::create_dir_all(&root).is_ok());
    let socket = root.join("main.sock");
    write_text_file(&socket, "keep\n");

    assert!(remove_stale_agent_terminal_socket(&socket).is_err());
    assert_eq!(fs::read_to_string(&socket).unwrap_or_default(), "keep\n");
}

#[test]
fn remove_stale_agent_terminal_socket_rejects_symlink_parent_without_removing_target_socket()
-> Result<(), Box<dyn std::error::Error>> {
    let root = clean_test_dir("ctx-agent-terminal-remove-parent-symlink");
    let outside = clean_test_dir("ctx-agent-terminal-remove-parent-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    let outside_socket = outside.join("main.sock");
    let listener = std::os::unix::net::UnixListener::bind(&outside_socket)?;
    assert!(std::os::unix::fs::symlink(&outside, root.join("runtime")).is_ok());

    let Err(error) = remove_stale_agent_terminal_socket(&root.join("runtime").join("main.sock"))
    else {
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
fn socket_bind_path_rejects_symlink_parent() {
    let root = clean_test_dir("ctx-terminal-socket-bind-parent-symlink");
    let outside = clean_test_dir("ctx-terminal-socket-bind-parent-symlink-outside");
    assert!(fs::create_dir_all(&root).is_ok());
    assert!(fs::create_dir_all(outside.join("runtime")).is_ok());
    assert!(std::os::unix::fs::symlink(
        outside.join("runtime").join("main.sock"),
        outside.join("main.sock")
    )
    .is_ok());
    assert!(std::os::unix::fs::symlink(&outside, root.join("visible")).is_ok());

    let socket = root.join("visible").join("main.sock");

    assert_eq!(socket_bind_path(&socket), socket);
}

#[test]
fn provider_oauth_uses_absolute_curl_path() {
    let command = ctx_provider_curl_command();
    assert_eq!(command.get_program(), CTX_PROVIDER_CURL_BIN);
    assert_eq!(
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>(),
        ["-q", "--config", "-"]
    );
    assert!(command.get_envs().next().is_none());
}

#[test]
fn provider_oauth_curl_quote_rejects_line_breaks() {
    assert!(curl_config_quote("https://oauth.example/token").is_ok());
    assert!(curl_config_quote("https://oauth.example/token\noutput = /tmp/leak").is_err());
    assert!(curl_config_quote("grant_type=refresh_token\rheader = injected").is_err());
    assert!(curl_config_quote("Authorization: Bearer \u{1b}]52;c;payload").is_err());
    assert!(curl_config_quote("abc\0def").is_err());
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
fn oauth_callback_parser_requires_http_version() {
    let parsed = parse_oauth_callback_params("GET /callback?code=ok&state=s\n\n", "/callback");

    assert!(matches!(parsed, Err(ref error) if error.code == 2));
}

#[test]
fn oauth_callback_parser_rejects_extra_request_line_fields() {
    let parsed =
        parse_oauth_callback_params("GET /callback?code=ok&state=s HTTP/1.1 extra\n\n", "/callback");

    assert!(matches!(parsed, Err(ref error) if error.code == 2));
}

#[test]
fn oauth_callback_parser_accepts_http_1_request_line() {
    let parsed =
        parse_oauth_callback_params("GET /callback?code=ok&state=s HTTP/1.1\r\n\r\n", "/callback");

    assert!(matches!(
        parsed,
        Ok(ref params)
            if params.code.as_deref() == Some("ok")
                && params.state.as_deref() == Some("s")
    ));
}

#[test]
fn oauth_callback_parser_rejects_repeated_code() {
    let parsed =
        parse_oauth_callback_params("GET /callback?code=one&code=two HTTP/1.1\r\n\r\n", "/callback");

    assert!(matches!(parsed, Err(ref error) if error.code == 2));
}

#[test]
fn oauth_callback_parser_rejects_repeated_state() {
    let parsed = parse_oauth_callback_params(
        "GET /callback?code=ok&state=one&state=two HTTP/1.1\r\n\r\n",
        "/callback",
    );

    assert!(matches!(parsed, Err(ref error) if error.code == 2));
}

#[test]
fn oauth_callback_parser_rejects_empty_code() {
    let parsed =
        parse_oauth_callback_params("GET /callback?code=&state=s HTTP/1.1\r\n\r\n", "/callback");

    assert!(matches!(parsed, Err(ref error) if error.code == 2));
}

#[test]
fn oauth_callback_parser_rejects_empty_state() {
    let parsed =
        parse_oauth_callback_params("GET /callback?code=ok&state= HTTP/1.1\r\n\r\n", "/callback");

    assert!(matches!(parsed, Err(ref error) if error.code == 2));
}

#[test]
fn current_session_name_falls_back_to_default_when_index_is_unreadable() {
    let root = clean_test_dir("ctx-current-session-unreadable");
    let index = root.join("index");
    assert!(fs::create_dir_all(&index).is_ok(), "failed to create session index directory");
    let current = index.join("current");
    assert!(fs::write(&current, "custom\n").is_ok(), "failed to write current session override");
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
