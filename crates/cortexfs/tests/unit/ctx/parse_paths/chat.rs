#[test]
fn terminal_connect_error_classifies_socket_failures() {
    let socket = Path::new("/tmp/cortexfs-terminal.sock");
    let missing = terminal_connect_cli_error(
        socket,
        "executor",
        "test",
        &cortexfs::runtime::terminal::broker::BrokerProtocolError::Io(std::io::Error::from(
            std::io::ErrorKind::NotFound,
        )),
    );
    assert!(missing.message.contains("terminal is not running"));
    assert!(
        missing
            .message
            .contains("ctx agent start executor --session test")
    );

    let refused = terminal_connect_cli_error(
        socket,
        "executor",
        "test",
        &cortexfs::runtime::terminal::broker::BrokerProtocolError::Io(std::io::Error::from(
            std::io::ErrorKind::ConnectionRefused,
        )),
    );
    assert!(
        refused
            .message
            .contains("terminal socket exists but has no listener")
    );
    assert!(!refused.message.contains("terminal is not running"));
}

#[test]
fn top_level_send_uses_agent_send_request_shape() {
    let root = clean_test_dir("ctx-top-level-send-agent-shape");
    let agent_dir = root.join("agent").join("executor.d");
    assert!(fs::create_dir_all(&agent_dir).is_ok());
    assert!(fs::write(agent_dir.join("cwd"), "/workspace\n").is_ok());
    let server = spawn_agent_socket_request_capture(&root, "executor");

    let result = run(vec![
        std::ffi::OsString::from("--root"),
        root.as_os_str().to_os_string(),
        std::ffi::OsString::from("send"),
        std::ffi::OsString::from("executor"),
        std::ffi::OsString::from("hello"),
    ]);

    assert!(matches!(result, Ok(code) if code == ExitCode::SUCCESS));
    let request = server.join();
    assert!(request.is_ok());
    let Ok(request) = request else {
        return;
    };
    assert!(request.contains("\"op\":\"send\""));
    assert!(request.contains("\"session\":\"default\""));
    assert!(request.contains("\"scope\":\"private\""));
    assert!(request.contains("\"cwd\":\"/workspace\""));
    assert!(request.contains("\"input\":\"hello\""));
}

#[test]
fn top_level_send_defaults_cwd_to_workspace() {
    let root = clean_test_dir("ctx-top-level-send-default-cwd");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    let server = spawn_agent_socket_request_capture(&root, "executor");

    let result = run(vec![
        std::ffi::OsString::from("--root"),
        root.as_os_str().to_os_string(),
        std::ffi::OsString::from("send"),
        std::ffi::OsString::from("executor"),
        std::ffi::OsString::from("hello"),
    ]);

    assert!(matches!(result, Ok(code) if code == ExitCode::SUCCESS));
    let request = server.join();
    assert!(request.is_ok());
    let Ok(request) = request else {
        return;
    };
    assert!(request.contains("\"cwd\":\"/workspace\""));
}

#[test]
fn top_level_send_ignores_external_session_workspace() {
    let root = clean_test_dir("ctx-top-level-send-session-workspace");
    let workspace = clean_test_dir("ctx-top-level-send-session-workspace-source");
    let agent_dir = root.join("agent").join("executor.d");
    let session = root
        .join("home")
        .join(current_uid_for_test())
        .join("agent")
        .join("executor")
        .join("session")
        .join("default");
    assert!(fs::create_dir_all(&agent_dir).is_ok());
    assert!(fs::create_dir_all(&session).is_ok());
    assert!(fs::write(agent_dir.join("cwd"), "/workspace\n").is_ok());
    assert!(
        fs::write(
            session.join("workspace"),
            format!("{}\n", workspace.display())
        )
        .is_ok()
    );
    let server = spawn_agent_socket_request_capture(&root, "executor");

    let result = run(vec![
        std::ffi::OsString::from("--root"),
        root.as_os_str().to_os_string(),
        std::ffi::OsString::from("send"),
        std::ffi::OsString::from("executor"),
        std::ffi::OsString::from("hello"),
    ]);

    assert!(matches!(result, Ok(code) if code == ExitCode::SUCCESS));
    let request = server.join();
    assert!(request.is_ok());
    let Ok(request) = request else {
        return;
    };
    assert!(request.contains("\"cwd\":\"/workspace\""));
    assert!(!request.contains("\"workspace\""));
    assert!(!request.contains(&workspace.display().to_string()));
}

#[test]
fn top_level_send_ignores_root_session_workspace() {
    let root = clean_test_dir("ctx-top-level-send-invalid-session-workspace");
    let agent_dir = root.join("agent").join("executor.d");
    let session = root
        .join("home")
        .join(current_uid_for_test())
        .join("agent")
        .join("executor")
        .join("session")
        .join("default");
    assert!(fs::create_dir_all(&agent_dir).is_ok());
    assert!(fs::create_dir_all(&session).is_ok());
    assert!(fs::write(agent_dir.join("cwd"), "/workspace\n").is_ok());
    assert!(fs::write(session.join("workspace"), "/\n").is_ok());
    let server = spawn_agent_socket_request_capture(&root, "executor");

    let result = run(vec![
        std::ffi::OsString::from("--root"),
        root.as_os_str().to_os_string(),
        std::ffi::OsString::from("send"),
        std::ffi::OsString::from("executor"),
        std::ffi::OsString::from("hello"),
    ]);

    assert!(matches!(result, Ok(code) if code == ExitCode::SUCCESS));
    let request = server.join();
    assert!(request.is_ok());
    let Ok(request) = request else {
        return;
    };
    assert!(request.contains("\"cwd\":\"/workspace\""));
    assert!(!request.contains("\"workspace\""));
}

#[test]
fn top_level_resume_uses_agent_resume_request_shape() {
    let root = clean_test_dir("ctx-top-level-resume-agent-shape");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    let session = root
        .join("home")
        .join(current_uid_for_test())
        .join("agent")
        .join("executor")
        .join("session")
        .join("default");
    assert!(fs::create_dir_all(&session).is_ok());
    let current = std::env::current_dir();
    assert!(current.is_ok());
    let Ok(current) = current else {
        return;
    };
    assert!(fs::write(session.join("workspace"), format!("{}\n", current.display())).is_ok());
    let server = spawn_agent_socket_request_capture(&root, "executor");

    let result = run(vec![
        std::ffi::OsString::from("--root"),
        root.as_os_str().to_os_string(),
        std::ffi::OsString::from("resume"),
        std::ffi::OsString::from("executor"),
    ]);

    assert!(matches!(result, Ok(code) if code == ExitCode::SUCCESS));
    let request = server.join();
    assert!(request.is_ok());
    let Ok(request) = request else {
        return;
    };
    assert!(request.contains("\"op\":\"resume\""));
    assert!(request.contains("\"session\":\"default\""));
    assert!(!request.contains("\"scope\""));
    assert!(!request.contains("\"input\""));
}

fn spawn_agent_socket_request_capture(root: &Path, agent: &str) -> std::thread::JoinHandle<String> {
    let socket = root.join("agent").join(format!("{agent}.sock"));
    let listener = std::os::unix::net::UnixListener::bind(&socket);
    assert!(listener.is_ok());
    let Ok(listener) = listener else {
        return std::thread::spawn(String::new);
    };

    std::thread::spawn(move || {
        let accepted = listener.accept();
        assert!(accepted.is_ok());
        let Ok((mut stream, _addr)) = accepted else {
            return String::new();
        };
        let mut request = String::new();
        assert!(std::io::Read::read_to_string(&mut stream, &mut request).is_ok());
        assert!(std::io::Write::write_all(&mut stream, b"{\"type\":\"done\"}\n").is_ok());
        request
    })
}

#[test]
fn buffered_agent_renderer_keeps_assistant_output_atomic() {
    let input = concat!(
        "{\"type\":\"delta\",\"text\":\"\\u4f60\"}\n",
        "{\"type\":\"tool_call\",\"name\":\"tsh\"}\n",
        "{\"type\":\"message\",\"role\":\"tool\",\"name\":\"tsh\",\"content\":[{\"type\":\"tool_result\",\"content\":\"abc\"}]}\n",
        "{\"type\":\"delta\",\"text\":\"\\u597d\"}\n",
        "{\"type\":\"done\"}\n",
        "{\"type\":\"error\",\"code\":\"EIO\",\"message\":\"boom\"}\n",
    );

    let rendered = collect_agent_events_buffered(std::io::Cursor::new(input));

    assert!(matches!(
        rendered,
        Ok(ref rendered)
            if rendered.output == "\u{4f60}\u{597d}\n"
                && rendered.diagnostics
                    == vec![
                        "tool tsh running".to_owned(),
                        "tool tsh done 3 bytes ~1 tokens\n  result: abc".to_owned(),
                        "error EIO: boom".to_owned()
                    ]
                && rendered.exit_code == 1
    ));
}

#[test]
fn buffered_agent_renderer_prints_assistant_content_array_message() {
    let input = concat!(
        "{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"工具已执行\"}]}\n",
        "{\"type\":\"done\",\"status\":\"ok\"}\n",
    );

    let rendered = collect_agent_events_buffered(std::io::Cursor::new(input));

    assert!(matches!(
        rendered,
        Ok(ref rendered)
            if rendered.output == "工具已执行\n"
                && rendered.diagnostics.is_empty()
                && rendered.exit_code == 0
    ));
}

#[test]
fn buffered_agent_renderer_reports_token_delta_and_total() {
    let input = concat!(
        "{\"type\":\"usage\",\"input_tokens\":10,\"output_tokens\":2}\n",
        "{\"type\":\"usage\",\"input_tokens\":4,\"output_tokens\":3}\n",
    );

    let rendered = collect_agent_events_buffered(std::io::Cursor::new(input));

    assert!(matches!(
        rendered,
        Ok(ref rendered)
            if rendered.diagnostics
                == vec![
                    "tokens in +10/10 out +2/2".to_owned(),
                    "tokens in +4/14 out +3/5".to_owned(),
                ]
    ));
}

#[test]
fn agent_renderer_waiting_diagnostic_is_readable() {
    assert_eq!(
        waiting_diagnostic(12),
        "agent waiting 12s for first event..."
    );
}

#[test]
fn debug_agent_send_request_marks_socket_frame() {
    let request = agent_send_request_json("run-1", "default", "/workspace", "hello", true);

    assert!(request.contains(r#""debug":true"#));
    assert!(!request.contains(r#""workspace""#));
    assert!(request.ends_with('\n'));
}

#[test]
fn normal_agent_send_request_does_not_mark_socket_frame() {
    let request = agent_send_request_json("run-1", "default", "/workspace", "hello", false);

    assert!(!request.contains(r#""debug""#));
    assert!(!request.contains(r#""workspace""#));
    assert!(request.ends_with('\n'));
}
