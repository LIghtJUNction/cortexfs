#[test]
fn agent_executable_socket_runtime_returns_visible_message() {
    let root = reference_tree("agent-executable-socket-runtime");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
run="$CTX_RUN_ID"
if [ -z "$run" ]; then
  run="r1"
fi
input="$*"
printf '{"type":"start","run":"%s","model":"debug/echo"}\n' "$run"
printf '{"type":"delta","run":"%s","text":"%s"}\n' "$run" "$input"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$run"
"#,
    );
    set_file_mode(&agent_executable, 0o755);
    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);

    assert!(
        client
            .write_all(
                br#"{"op":"send","id":"msg-1","session":"default","input":"hi"}
"#,
            )
            .is_ok()
    );
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            ctx_root: &root,
            source_root: &root,
            identity: view.identity(),
            env: view.env(),
            session_root: &session_root,
            default_cwd: "/work",
            model: Some("debug/echo"),
            network_allowed: false,
            agent_name: "coder",
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Direct,
        },
    );
    let outcome = ok!(outcome);
    assert_eq!(outcome.frames().len(), 3);
    assert!(outcome.jsonl().contains("\"type\":\"start\""));
    assert!(outcome.jsonl().contains("\"type\":\"delta\""));
    assert!(outcome.jsonl().contains("\"text\":\"hi\""));
    assert!(outcome.jsonl().contains("\"type\":\"done\""));

    let mut buffer = [0_u8; 512];
    let read = client.read(&mut buffer);
    let read = ok!(read);
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"type\":\"delta\""));
    assert!(response.contains("\"text\":\"hi\""));
    assert_file_text(&session_root.join("default").join("latest.md"), "hi\n");
}

#[test]
fn agent_executable_socket_runtime_records_terminal_provider_error() {
    let root = reference_tree("agent-executable-provider-error");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
printf '{"type":"start","run":"%s","model":"main"}\n' "$CTX_RUN_ID"
printf '{"type":"error","run":"%s","code":"EIO","message":"provider request failed with exit status: 22: curl: (22) The requested URL returned error: 502"}\n' "$CTX_RUN_ID"
printf '{"type":"done","run":"%s","status":"error"}\n' "$CTX_RUN_ID"
"#,
    );
    set_file_mode(&agent_executable, 0o755);
    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);
    assert!(
        client
            .write_all(
                br#"{"op":"send","id":"provider-error-1","session":"default","input":"hi"}
"#,
            )
            .is_ok()
    );
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            ctx_root: &root,
            source_root: &root,
            identity: view.identity(),
            env: view.env(),
            session_root: &session_root,
            default_cwd: "/work",
            model: Some("main"),
            network_allowed: false,
            agent_name: "coder",
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Direct,
        },
    );
    let outcome = ok!(outcome);
    assert!(outcome.jsonl().contains(r#""type":"error""#));
    assert!(outcome.jsonl().contains(r#""status":"error""#));
    let session = session_root.join("default");
    let events = fs::read_to_string(session.join("events.jsonl")).unwrap_or_default();
    assert!(events.lines().any(|line| {
        line.contains(r#""type":"error""#)
            && line.contains(r#""run":"provider-error-1""#)
            && line.contains("502")
    }));
    assert!(events.lines().any(|line| {
        line.contains(r#""type":"done""#)
            && line.contains(r#""run":"provider-error-1""#)
            && line.contains(r#""status":"error""#)
    }));
    assert_file_text(&session.join("state"), "error\n");
}

#[test]
fn agent_executable_socket_runtime_keeps_error_after_partial_delta() {
    let root = reference_tree("agent-executable-partial-provider-error");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
printf '{"type":"start","run":"%s","model":"main"}\n' "$CTX_RUN_ID"
printf '{"type":"delta","run":"%s","text":"partial"}\n' "$CTX_RUN_ID"
printf '{"type":"error","run":"%s","code":"EIO","message":"provider request failed with exit status: 22: curl: (22) The requested URL returned error: 502"}\n' "$CTX_RUN_ID"
printf '{"type":"done","run":"%s","status":"error"}\n' "$CTX_RUN_ID"
"#,
    );
    set_file_mode(&agent_executable, 0o755);
    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);
    assert!(
        client
            .write_all(
                br#"{"op":"send","id":"partial-error-1","session":"default","input":"hi"}
"#,
            )
            .is_ok()
    );
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            ctx_root: &root,
            source_root: &root,
            identity: view.identity(),
            env: view.env(),
            session_root: &session_root,
            default_cwd: "/work",
            model: Some("main"),
            network_allowed: false,
            agent_name: "coder",
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Direct,
        },
    );
    let outcome = ok!(outcome);
    assert!(outcome.jsonl().contains(r#""text":"partial""#));
    let session = session_root.join("default");
    let events = fs::read_to_string(session.join("events.jsonl")).unwrap_or_default();
    assert!(events.lines().any(|line| {
        line.contains(r#""type":"error""#)
            && line.contains(r#""run":"partial-error-1""#)
            && line.contains("502")
    }));
    assert!(events.lines().any(|line| {
        line.contains(r#""type":"done""#)
            && line.contains(r#""run":"partial-error-1""#)
            && line.contains(r#""status":"error""#)
    }));
    assert!(!events.lines().any(|line| {
        line.contains(r#""type":"done""#)
            && line.contains(r#""run":"partial-error-1""#)
            && line.contains(r#""status":"ok""#)
    }));
    assert_file_text(&session.join("state"), "error\n");
}

#[test]
fn agent_executable_socket_runtime_wraps_plain_text_after_visible_events() {
    let root = reference_tree("agent-plain-after-event");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
printf '{"type":"delta","run":"%s","text":"before"}\n' "$CTX_RUN_ID"
printf 'plain followup\n'
"#,
    );
    set_file_mode(&agent_executable, 0o755);

    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);
    assert!(
        client
            .write_all(
                br#"{"op":"send","id":"msg-1","session":"default","input":"hi"}
"#,
            )
            .is_ok()
    );
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            ctx_root: &root,
            source_root: &root,
            identity: view.identity(),
            env: view.env(),
            session_root: &session_root,
            default_cwd: "/work",
            model: Some("debug/echo"),
            network_allowed: false,
            agent_name: "coder",
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Direct,
        },
    );
    let outcome = ok!(outcome);
    assert!(outcome.jsonl().contains(r#""text":"before""#));
    assert!(outcome.jsonl().contains(r#""text":"plain followup""#));
    assert_file_text(
        &session_root.join("default").join("latest.md"),
        "beforeplain followup\n",
    );
}

#[test]
fn agent_executable_socket_runtime_records_untrusted_debug_frame_as_text() {
    let root = reference_tree("agent-untrusted-debug-frame");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
printf '{"type":"delta","run":"%s","text":"before"}\n' "$CTX_RUN_ID"
printf '{"type":"debug","elapsed_ms":0,"stage":"ATTACKER_UNAUDITED_SECRET"}\n'
printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
"#,
    );
    set_file_mode(&agent_executable, 0o755);

    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);
    assert!(client
        .write_all(
            br#"{"op":"send","id":"msg-1","session":"default","input":"hi"}
"#,
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            ctx_root: &root,
            source_root: &root,
            identity: view.identity(),
            env: view.env(),
            session_root: &session_root,
            default_cwd: "/work",
            model: Some("debug/echo"),
            network_allowed: false,
            agent_name: "coder",
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Direct,
        },
    );
    let outcome = ok!(outcome);
    assert!(!outcome.jsonl().contains(r#""type":"debug""#));
    assert!(outcome.jsonl().contains("ATTACKER_UNAUDITED_SECRET"));
    assert!(outcome
        .jsonl()
        .contains(r#""text":"{\"type\":\"debug\",\"elapsed_ms\":0,\"stage\":\"ATTACKER_UNAUDITED_SECRET\"}""#));
    assert_file_text(
        &session_root.join("default").join("latest.md"),
        "before{\"type\":\"debug\",\"elapsed_ms\":0,\"stage\":\"ATTACKER_UNAUDITED_SECRET\"}\n",
    );
}

#[test]
fn agent_executable_socket_runtime_rejects_symlink_executable_without_running_target() {
    let root = reference_tree("agent-executable-socket-runtime-symlink");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let outside = clean_test_dir("agent-executable-socket-runtime-symlink-outside");
    let target = outside.join("coder-target");
    let marker = outside.join("ran");
    write_text_file(
        &target,
        &format!(
            "#!/bin/sh\nprintf ran > {}\nprintf '{{\"type\":\"done\",\"run\":\"%s\",\"status\":\"ok\"}}\\n' \"$CTX_RUN_ID\"\n",
            marker.display()
        ),
    );
    set_file_mode(&target, 0o755);
    let agent_executable = root.join("agent").join("coder");
    assert!(fs::remove_file(&agent_executable).is_ok());
    assert!(symlink(&target, &agent_executable).is_ok());

    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);
    assert!(
        client
            .write_all(
                br#"{"op":"send","id":"msg-1","session":"default","input":"hi"}
"#,
            )
            .is_ok()
    );
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            ctx_root: &root,
            source_root: &root,
            identity: view.identity(),
            env: view.env(),
            session_root: &session_root,
            default_cwd: "/work",
            model: Some("debug/echo"),
            network_allowed: false,
            agent_name: "coder",
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Direct,
        },
    );

    assert_eq!(outcome, Err(SocketRuntimeError::InvalidAgentExecutable));
    assert!(!marker.exists());
}

#[test]
fn agent_executable_socket_runtime_rejects_oversized_output_frame() {
    let root = reference_tree("agent-oversized-frame");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r"#!/bin/sh
head -c 262144 /dev/zero | tr '\0' x
printf '\n'
",
    );
    set_file_mode(&agent_executable, 0o755);

    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);
    assert!(
        client
            .write_all(
                br#"{"op":"send","id":"msg-1","session":"default","input":"hi"}
"#,
            )
            .is_ok()
    );
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            ctx_root: &root,
            source_root: &root,
            identity: view.identity(),
            env: view.env(),
            session_root: &session_root,
            default_cwd: "/work",
            model: Some("debug/echo"),
            network_allowed: false,
            agent_name: "coder",
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Direct,
        },
    );

    assert_eq!(outcome, Err(SocketRuntimeError::InvalidAgentOutput));
}

#[test]
fn agent_executable_socket_runtime_passes_source_root() {
    let root = reference_tree("agent-executable-socket-runtime-source-root");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
printf '{"type":"delta","run":"%s","text":"%s"}\n' "$CTX_RUN_ID" "$CTX_SOURCE"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
"#,
    );
    set_file_mode(&agent_executable, 0o755);

    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);
    assert!(
        client
            .write_all(
                br#"{"op":"send","id":"msg-1","session":"default","input":"hi"}
"#,
            )
            .is_ok()
    );
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
            ctx_root: &root,
            source_root: &root,
            identity: view.identity(),
            env: view.env(),
            session_root: &session_root,
            default_cwd: "/work",
            model: Some("debug/echo"),
            network_allowed: false,
            agent_name: "coder",
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Direct,
        },
    );
    let outcome = ok!(outcome);
    assert!(
        outcome
            .jsonl()
            .contains(&format!(r#""text":"{}""#, root.to_string_lossy()))
    );
}
