#[test]
fn agent_executable_socket_runtime_passes_history_messages() {
    let root = reference_tree("agent-executable-socket-runtime-history");
    let session_root = agent_session_root(&root, "coder");
    write_text_file(
        &session_root.join("default").join("messages.jsonl"),
        r#"{"content":"remember prior","role":"user"}
{"content":[{"text":"prior answer","type":"text"}],"role":"assistant"}
"#,
    );
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
history="$(/usr/bin/printf '%s' "$CTX_AGENT_HISTORY_MESSAGES" | /usr/bin/tr '\n' '|')"
printf '{"type":"delta","run":"%s","text":"%s"}\n' "$CTX_RUN_ID" "$history"
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
    assert!(outcome.jsonl().contains("- user: remember prior"));
    assert!(outcome.jsonl().contains("- assistant: prior answer"));
    assert!(!outcome.jsonl().contains("- user: hi"));
}

#[test]
fn agent_executable_socket_runtime_stops_child_after_cancel() {
    let root = reference_tree("agent-executable-socket-runtime-cancel");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
trap 'printf term > "$CTX_SOURCE/agent-terminated"; exit 0' TERM
printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
sh -c 'trap "" TERM; while [ ! -f "$CTX_SOURCE/release-agent" ]; do sleep 0.05; done; printf leaked > "$CTX_SOURCE/grandchild-leaked"' &
touch "$CTX_SOURCE/agent-ready"
while [ ! -f "$CTX_SOURCE/release-agent" ]; do
  sleep 0.05
done
printf '{"type":"delta","run":"%s","text":"too-late"}\n' "$CTX_RUN_ID"
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

    let cancel_root = session_root.clone();
    let ready_file = root.join("agent-ready");
    let cancel_thread = thread::spawn(move || {
        for _attempt in 0..50 {
            if ready_file.exists() {
                let cancel = handle_socket_request_frame(
                    &cancel_root,
                    "/work",
                    Some("debug/echo"),
                    r#"{"op":"cancel","id":"msg-1"}"#,
                );
                return cancel.is_ok();
            }
            thread::sleep(Duration::from_millis(20));
        }
        false
    });

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

    let joined = cancel_thread.join();
    assert!(joined.is_ok());
    let Ok(cancelled) = joined else {
        return;
    };
    assert!(cancelled);
    let outcome = ok!(outcome);
    assert!(!outcome.jsonl().contains("too-late"));
    assert_file_text(&session_root.join("default").join("state"), "cancelled\n");
    assert_file_text(&root.join("agent-terminated"), "term");
    assert!(fs::write(root.join("release-agent"), "").is_ok());
    thread::sleep(Duration::from_millis(200));
    assert!(!root.join("grandchild-leaked").exists());
}

#[test]
fn agent_executable_socket_runtime_preserves_jsonl_error_output() {
    let root = reference_tree("agent-executable-socket-runtime-error-output");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
printf '{"type":"error","run":"%s","code":"EHOSTDOWN","message":"model unavailable"}\n' "$CTX_RUN_ID"
printf '{"type":"done","run":"%s","status":"error"}\n' "$CTX_RUN_ID"
exit 1
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
    assert!(outcome.jsonl().contains("\"code\":\"EHOSTDOWN\""));
    assert!(
        outcome
            .jsonl()
            .contains("\"message\":\"model unavailable\"")
    );
    assert!(outcome.jsonl().contains("\"status\":\"error\""));

    let mut buffer = [0_u8; 512];
    let read = client.read(&mut buffer);
    let read = ok!(read);
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("\"code\":\"EHOSTDOWN\""));
    assert!(response.contains("\"message\":\"model unavailable\""));
}

#[test]
fn agent_executable_socket_runtime_reports_pre_event_process_failure() {
    let root = reference_tree("agent-pre-event-failure");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
"#,
    );
    set_file_mode(&agent_executable, 0o755);
    let failing_program = root.join("failing-bwrap");
    write_text_file(
        &failing_program,
        r"#!/bin/sh
printf 'bwrap: unable to bind /ctx/agent/coder\n' >&2
exit 1
",
    );
    set_file_mode(&failing_program, 0o755);

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
            execution: AgentExecutableSocketExecution::Bwrap {
                program: &failing_program,
                mount_table: view.mount_table(),
            },
        },
    );
    let outcome = ok!(outcome);
    assert!(outcome.jsonl().contains("\"code\":\"EIO\""));
    assert!(outcome.jsonl().contains("unable to bind"));
    assert!(outcome.jsonl().contains("\"status\":\"error\""));

    let mut buffer = [0_u8; 512];
    let read = client.read(&mut buffer);
    let read = ok!(read);
    let Some(bytes) = buffer.get(..read) else {
        return;
    };
    let response = String::from_utf8_lossy(bytes);
    assert!(response.contains("unable to bind"));
    assert!(!response.contains(r#""message":"EIO""#));
}

