#[test]
fn terminal_connect_error_classifies_socket_failures() {
    let socket = Path::new("/tmp/cortexfs-terminal.sock");
    let missing = terminal_connect_cli_error(
        socket,
        "coder",
        "test",
        &std::io::Error::from(std::io::ErrorKind::NotFound),
    );
    assert!(missing.message.contains("terminal is not running"));
    assert!(missing.message.contains("ctx agent start coder --session test"));

    let refused = terminal_connect_cli_error(
        socket,
        "coder",
        "test",
        &std::io::Error::from(std::io::ErrorKind::ConnectionRefused),
    );
    assert!(refused
        .message
        .contains("terminal socket exists but has no listener"));
    assert!(!refused.message.contains("terminal is not running"));
}

#[test]
fn agent_repl_editor_enables_terminal_signals() {
    assert!(agent_repl_editor_config().enable_signals());
}

#[test]
fn agent_repl_prompt_and_model_summary_are_chat_oriented() {
    let root = clean_test_dir("ctx-agent-repl-model-summary");
    assert!(fs::create_dir_all(root.join("agent/coder.d")).is_ok());
    let session = root
        .join("home")
        .join(std::process::Command::new("id")
            .arg("-u")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|uid| uid.trim().to_owned())
            .filter(|uid| !uid.is_empty())
            .unwrap_or_else(|| "1000".to_owned()))
        .join("agent/coder/session/default");
    assert!(fs::create_dir_all(&session).is_ok());
    assert!(fs::create_dir_all(root.join("model")).is_ok());
    assert!(fs::write(root.join("agent/coder.d/model"), "main\n").is_ok());
    assert!(fs::write(session.join("workspace"), "/repo\n").is_ok());
    assert!(
        std::os::unix::fs::symlink("/ctx/model/localhost/gpt-5.4-mini", root.join("model/main"))
            .is_ok()
    );

    assert_eq!(
        agent_repl_prompt(false, "coder", "default"),
        "ctx agent coder/default ❯ "
    );
    assert_eq!(
        agent_repl_model_summary(false, &root, "coder"),
        Ok("main -> /ctx/model/localhost/gpt-5.4-mini (missing)".to_owned())
    );
    let banner = agent_repl_banner_lines(false, &root, "coder", "default");
    assert!(matches!(banner, Ok(ref lines) if lines.iter().any(|line| line == " Workspace: /repo")));
    assert!(AGENT_REPL_COMMANDS.contains("/help"));
    assert!(AGENT_REPL_COMMANDS.contains("/clear"));
}

#[test]
fn agent_repl_model_summary_defaults_missing_worker_model_to_spark() {
    let root = clean_test_dir("ctx-agent-repl-worker-default-model");
    assert!(fs::create_dir_all(root.join("agent/worker.d")).is_ok());

    assert_eq!(
        agent_repl_model_summary(false, &root, "worker"),
        Ok("api.lmm.best/gpt-5.3-codex-spark".to_owned())
    );
}

#[test]
fn agent_repl_model_summary_rejects_invalid_model() {
    let root = clean_test_dir("ctx-agent-repl-invalid-model");
    assert!(fs::create_dir_all(root.join("agent/worker.d")).is_ok());
    assert!(fs::write(root.join("agent/worker.d/model"), "bad/model/name\n").is_ok());

    assert!(matches!(
        agent_repl_model_summary(false, &root, "worker"),
        Err(ref error)
            if error.code == 2
                && error.message == "invalid agent model for worker: bad/model/name"
    ));
}

#[test]
fn agent_repl_model_summary_defaults_worker_prefix_to_spark() {
    let root = clean_test_dir("ctx-agent-repl-worker-prefix-default-model");
    assert!(fs::create_dir_all(root.join("agent/worker-fast.d")).is_ok());

    assert_eq!(
        agent_repl_model_summary(false, &root, "worker-fast"),
        Ok("api.lmm.best/gpt-5.3-codex-spark".to_owned())
    );
}

#[test]
fn agent_repl_model_summary_rejects_symlink_model_directory() {
    let root = clean_test_dir("ctx-agent-repl-model-summary-symlink-model");
    let outside = clean_test_dir("ctx-agent-repl-model-summary-symlink-model-outside");
    assert!(fs::create_dir_all(root.join("agent/coder.d")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::write(root.join("agent/coder.d/model"), "main\n").is_ok());
    assert!(
        std::os::unix::fs::symlink("/ctx/model/localhost/gpt-5.4-mini", outside.join("main"))
            .is_ok()
    );
    assert!(std::os::unix::fs::symlink(&outside, root.join("model")).is_ok());

    assert_eq!(
        agent_repl_model_summary(false, &root, "coder"),
        Ok("main (missing alias)".to_owned())
    );
}

#[test]
fn agent_repl_model_summary_does_not_follow_symlink_alias_target() {
    let root = clean_test_dir("ctx-agent-repl-model-summary-symlink-target");
    let outside = clean_test_dir("ctx-agent-repl-model-summary-symlink-target-outside");
    assert!(fs::create_dir_all(root.join("agent/coder.d")).is_ok());
    assert!(fs::create_dir_all(root.join("model/localhost")).is_ok());
    assert!(fs::create_dir_all(&outside).is_ok());
    assert!(fs::write(root.join("agent/coder.d/model"), "main\n").is_ok());
    assert!(
        std::os::unix::fs::symlink("/ctx/model/localhost/gpt-5.4-mini", root.join("model/main"))
            .is_ok()
    );
    assert!(std::os::unix::fs::symlink(
        &outside,
        root.join("model/localhost/gpt-5.4-mini")
    )
    .is_ok());

    assert_eq!(
        agent_repl_model_summary(false, &root, "coder"),
        Ok("main -> /ctx/model/localhost/gpt-5.4-mini (missing)".to_owned())
    );
}

#[test]
fn agent_repl_exits_on_interrupt_signal_errors() {
    assert!(agent_repl_should_exit_on_readline_error(
        &rustyline::error::ReadlineError::Interrupted
    ));
    assert!(agent_repl_should_exit_on_readline_error(
        &rustyline::error::ReadlineError::Signal(rustyline::error::Signal::Interrupt)
    ));
    assert!(agent_repl_should_exit_on_readline_error(
        &rustyline::error::ReadlineError::Eof
    ));
}

#[test]
fn top_level_send_uses_agent_send_request_shape() {
    let root = clean_test_dir("ctx-top-level-send-agent-shape");
    let agent_dir = root.join("agent").join("coder.d");
    assert!(fs::create_dir_all(&agent_dir).is_ok());
    assert!(fs::write(agent_dir.join("cwd"), "/workspace\n").is_ok());
    let server = spawn_agent_socket_request_capture(&root, "coder");

    let result = run(vec![
        std::ffi::OsString::from("--root"),
        root.as_os_str().to_os_string(),
        std::ffi::OsString::from("send"),
        std::ffi::OsString::from("coder"),
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
    let server = spawn_agent_socket_request_capture(&root, "coder");

    let result = run(vec![
        std::ffi::OsString::from("--root"),
        root.as_os_str().to_os_string(),
        std::ffi::OsString::from("send"),
        std::ffi::OsString::from("coder"),
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
fn top_level_send_uses_session_workspace_hint() {
    let root = clean_test_dir("ctx-top-level-send-session-workspace");
    let workspace = clean_test_dir("ctx-top-level-send-session-workspace-source");
    let agent_dir = root.join("agent").join("coder.d");
    let session = root
        .join("home")
        .join(current_uid_for_test())
        .join("agent")
        .join("coder")
        .join("session")
        .join("default");
    assert!(fs::create_dir_all(&agent_dir).is_ok());
    assert!(fs::create_dir_all(&session).is_ok());
    assert!(fs::write(agent_dir.join("cwd"), "/workspace\n").is_ok());
    assert!(fs::write(session.join("workspace"), format!("{}\n", workspace.display())).is_ok());
    let server = spawn_agent_socket_request_capture(&root, "coder");

    let result = run(vec![
        std::ffi::OsString::from("--root"),
        root.as_os_str().to_os_string(),
        std::ffi::OsString::from("send"),
        std::ffi::OsString::from("coder"),
        std::ffi::OsString::from("hello"),
    ]);

    assert!(matches!(result, Ok(code) if code == ExitCode::SUCCESS));
    let request = server.join();
    assert!(request.is_ok());
    let Ok(request) = request else {
        return;
    };
    assert!(request.contains("\"cwd\":\"/workspace\""));
    assert!(request.contains(&format!(r#""workspace":"{}""#, workspace.display())));
}

#[test]
fn top_level_resume_uses_agent_resume_request_shape() {
    let root = clean_test_dir("ctx-top-level-resume-agent-shape");
    assert!(fs::create_dir_all(root.join("agent")).is_ok());
    let server = spawn_agent_socket_request_capture(&root, "coder");

    let result = run(vec![
        std::ffi::OsString::from("--root"),
        root.as_os_str().to_os_string(),
        std::ffi::OsString::from("resume"),
        std::ffi::OsString::from("coder"),
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

fn spawn_agent_socket_request_capture(
    root: &Path,
    agent: &str,
) -> std::thread::JoinHandle<String> {
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
        assert!(
            std::io::Write::write_all(&mut stream, b"{\"type\":\"done\"}\n").is_ok()
        );
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
                        "tool tsh done 3 bytes".to_owned(),
                        "error EIO: boom".to_owned()
                    ]
                && rendered.exit_code == 1
                && !rendered.interrupted
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
                && !rendered.interrupted
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
    assert_eq!(waiting_diagnostic(12), "waiting 12s for agent event");
}

#[test]
fn debug_tool_line_reports_current_names_and_changes() {
    assert_eq!(
        format_debug_tool_line(None, &["fs.read".to_owned(), "tsh".to_owned()]),
        "[debug tools] = fs.read tsh"
    );
    assert_eq!(
        format_debug_tool_line(
            Some(&["fs.read".to_owned(), "tsh".to_owned()]),
            &["fs.read".to_owned(), "fs.write".to_owned()]
        ),
        "[debug tools] +fs.write -tsh = fs.read fs.write"
    );
}

#[test]
fn debug_agent_send_request_marks_socket_frame() {
    let request = agent_send_request_json(
        "run-1",
        "default",
        "/workspace",
        Some("/repo"),
        "hello",
        true,
    );

    assert!(request.contains(r#""debug":true"#));
    assert!(request.contains(r#""workspace":"/repo""#));
    assert!(request.ends_with('\n'));
}

#[test]
fn normal_agent_send_request_does_not_mark_socket_frame() {
    let request = agent_send_request_json("run-1", "default", "/workspace", None, "hello", false);

    assert!(!request.contains(r#""debug""#));
    assert!(!request.contains(r#""workspace""#));
    assert!(request.ends_with('\n'));
}
