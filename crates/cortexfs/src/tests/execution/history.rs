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
IFS= read -r envelope || exit 2
history="$(printf '%s' "$envelope" | jq -r '.history_messages' | tr '\n' '|')"
tool_context="$(printf '%s' "$envelope" | jq -r '.tool_context')"
case "$tool_context" in
  *'Host workspace configuration: determined by agent policy'*) context="workspace-context-ok" ;;
  *) context="missing-workspace-context" ;;
esac
if [ -n "${CTX_WORKSPACE+x}" ]; then context="workspace-env-leaked"; fi
case "$tool_context" in *'/repo'*) context="workspace-path-leaked" ;; esac
printf '{"type":"delta","run":"%s","text":"%s %s"}\n' "$CTX_RUN_ID" "$history" "$context"
"#,
    );
    set_file_mode(&agent_executable, 0o755);

    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);
    assert!(
        client
            .write_all(
                br#"{"op":"send","id":"msg-1","session":"default","cwd":"/workspace","workspace":"/repo","input":"hi"}
"#,
            )
            .is_ok()
    );
    assert!(client.shutdown(Shutdown::Write).is_ok());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        direct_agent_runtime(&root, &view, &session_root, &agent_executable),
    );
    let outcome = ok!(outcome);
    assert!(outcome.jsonl().contains("- user: remember prior"));
    assert!(outcome.jsonl().contains("- assistant: prior answer"));
    assert!(outcome.jsonl().contains("workspace-context-ok"));
    assert!(!outcome.jsonl().contains("workspace-env-leaked"));
    assert!(!outcome.jsonl().contains("workspace-path-leaked"));
    assert!(!outcome.jsonl().contains("/repo"));
    assert!(!outcome.jsonl().contains("- user: hi"));
}

#[test]
fn agent_request_failures_are_terminal_without_failing_the_socket_runtime() {
    let root = reference_tree("agent-request-failure-terminal");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent/coder");
    write_text_file(
        &agent_executable,
        "#!/bin/sh\nIFS= read -r envelope || exit 2\nexit 1\n",
    );
    set_file_mode(&agent_executable, 0o755);
    let runtime = direct_agent_runtime(&root, &view, &session_root, &agent_executable);

    for request_id in ["failure-1", "failure-2"] {
        let (mut client, mut socket) = ok!(UnixStream::pair());
        assert!(
            writeln!(
                client,
                "{{\"op\":\"send\",\"id\":\"{request_id}\",\"session\":\"default\",\"input\":\"fail\"}}"
            )
            .is_ok()
        );
        assert!(client.shutdown(Shutdown::Write).is_ok());
        let outcome = serve_agent_executable_socket_stream_once(&mut socket, None, runtime);
        let outcome = ok!(outcome);
        let terminal = outcome
            .frames()
            .iter()
            .filter(|frame| {
                serde_json::from_str::<serde_json::Value>(frame).is_ok_and(|value| {
                    value.get("type").and_then(serde_json::Value::as_str) == Some("done")
                })
            })
            .collect::<Vec<_>>();
        assert_eq!(terminal.len(), 1);
        assert!(
            terminal
                .first()
                .is_some_and(|frame| frame.contains("\"status\":\"error\""))
        );
        assert!(outcome.frames().iter().any(|frame| {
            serde_json::from_str::<serde_json::Value>(frame).is_ok_and(|value| {
                value.get("type").and_then(serde_json::Value::as_str) == Some("error")
            })
        }));
        assert!(outcome.frames().iter().any(|frame| {
            serde_json::from_str::<serde_json::Value>(frame).is_ok_and(|value| {
                value.get("type").and_then(serde_json::Value::as_str) == Some("done")
                    && value.get("status").and_then(serde_json::Value::as_str) == Some("error")
            })
        }));
        assert_file_text(&session_root.join("default/state"), "error\n");
    }
}

#[test]
fn successful_agent_without_done_gets_canonical_terminal_frame() {
    let root = reference_tree("agent-success-canonical-done");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let executable = root.join("agent/coder");
    write_text_file(
        &executable,
        "#!/bin/sh\nprintf '{\"type\":\"delta\",\"run\":\"%s\",\"text\":\"ok\"}\\n' \"$CTX_RUN_ID\"\n",
    );
    set_file_mode(&executable, 0o755);
    let (mut client, mut socket) = ok!(UnixStream::pair());
    assert!(client
        .write_all(
            b"{\"op\":\"send\",\"id\":\"success-1\",\"session\":\"default\",\"input\":\"ok\"}\n"
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());
    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        direct_agent_runtime(&root, &view, &session_root, &executable),
    );
    let outcome = ok!(outcome);
    assert_eq!(
        outcome
            .frames()
            .iter()
            .filter(|frame| frame.contains("\"type\":\"done\""))
            .count(),
        1
    );
    assert!(outcome.frames().iter().any(|frame| {
        serde_json::from_str::<serde_json::Value>(frame).is_ok_and(|value| {
            value.get("type").and_then(serde_json::Value::as_str) == Some("done")
                && value.get("status").and_then(serde_json::Value::as_str) == Some("ok")
        })
    }));
    assert_file_text(&session_root.join("default/state"), "done\n");
}

#[test]
fn duplicate_agent_done_is_rejected_before_terminal_delivery() {
    let root = reference_tree("agent-duplicate-done");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let executable = root.join("agent/coder");
    write_text_file(
        &executable,
        "#!/bin/sh\nprintf '{\"type\":\"done\",\"run\":\"%s\",\"status\":\"ok\"}\\n' \"$CTX_RUN_ID\"\nprintf '{\"type\":\"done\",\"run\":\"%s\",\"status\":\"error\"}\\n' \"$CTX_RUN_ID\"\n",
    );
    set_file_mode(&executable, 0o755);
    let (mut client, mut socket) = ok!(UnixStream::pair());
    assert!(client
        .write_all(
            b"{\"op\":\"send\",\"id\":\"duplicate-1\",\"session\":\"default\",\"input\":\"x\"}\n"
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());
    let outcome = ok!(serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        direct_agent_runtime(&root, &view, &session_root, &executable),
    ));
    assert!(
        outcome
            .frames()
            .iter()
            .any(|frame| frame.contains("\"code\":\"EPROTO\""))
    );
    assert!(
        outcome
            .frames()
            .iter()
            .any(|frame| frame.contains("\"status\":\"error\""))
    );
    drop(socket);
    let mut response = String::new();
    assert!(client.read_to_string(&mut response).is_ok());
    assert!(response.contains("\"code\":\"EPROTO\""));
    assert_file_text(&session_root.join("default/state"), "error\n");
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
IFS= read -r envelope || exit 2
tool_context="$(printf '%s' "$envelope" | jq -r '.tool_context')"
if [ -n "${CTX_WORKSPACE+x}" ]; then touch "$CTX_SOURCE/workspace-env-leaked"; fi
case "$tool_context" in
  *'Host workspace configuration: determined by agent policy'*) ;;
  *) touch "$CTX_SOURCE/workspace-context-leaked" ;;
esac
sh -c 'trap "" TERM; while [ ! -f "$CTX_SOURCE/release-agent" ]; do sleep 0.05; done; printf leaked > "$CTX_SOURCE/grandchild-leaked"' &
touch "$CTX_SOURCE/agent-ready"
while [ ! -f "$CTX_SOURCE/release-agent" ]; do
  sleep 0.05
done
printf '{"type":"delta","run":"%s","text":"too-late"}\n' "$CTX_RUN_ID"
"#,
    );
    set_file_mode(&agent_executable, 0o755);

    let pair = UnixStream::pair();
    let (mut client, mut socket) = ok!(pair);
    assert!(
        client
            .write_all(
                br#"{"op":"send","id":"msg-1","session":"default","workspace":"/","input":"hi"}
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
        direct_agent_runtime(&root, &view, &session_root, &agent_executable),
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
    assert!(!root.join("workspace-env-leaked").exists());
    assert!(!root.join("workspace-context-leaked").exists());
    assert!(fs::write(root.join("release-agent"), "").is_ok());
    thread::sleep(Duration::from_millis(200));
    assert!(!root.join("grandchild-leaked").exists());
}

#[test]
fn sdk_envelope_cancel_stops_active_step_before_respawn() {
    let root = reference_tree("sdk-envelope-cancel-step");
    write_text_file(&root.join("agent/coder.d/abi"), "sdk-envelope-v1\n");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let executable = root.join("agent/coder");
    write_text_file(
        &executable,
        "#!/bin/sh\ntrap 'printf term > \"$CTX_SOURCE/envelope-terminated\"; exit 0' TERM\nIFS= read -r envelope\ntouch \"$CTX_SOURCE/envelope-ready\"\nsleep 10\nprintf '{\"type\":\"message\",\"run\":\"%s\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"late\"}]}\\n' \"$CTX_RUN_ID\"\n",
    );
    set_file_mode(&executable, 0o755);
    let cancel_root = session_root.clone();
    let ready = root.join("envelope-ready");
    let (mut client, mut socket) = ok!(UnixStream::pair());
    assert!(
        client
            .write_all(
                b"{\"op\":\"send\",\"id\":\"r1\",\"session\":\"default\",\"input\":\"wait\"}\n"
            )
            .is_ok()
    );
    assert!(client.shutdown(Shutdown::Write).is_ok());
    let cancel = thread::spawn(move || cancel_when_ready(&cancel_root, &ready));
    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        direct_agent_runtime(&root, &view, &session_root, &executable),
    );
    let cancelled = cancel.join();
    assert!(
        matches!(cancelled, Ok(true)),
        "cancel={cancelled:?} outcome={outcome:?} ready={}",
        view.home().join("tool-ready").exists()
    );
    let outcome = ok!(outcome);
    assert!(!outcome.jsonl().contains("late"));
    assert_file_text(&session_root.join("default/state"), "cancelled\n");
    assert_file_text(&root.join("envelope-terminated"), "term");
}

#[test]
fn sdk_envelope_cancel_during_tool_has_no_result_or_respawn() {
    let root = reference_tree("sdk-envelope-cancel-tool");
    write_text_file(&root.join("agent/coder.d/abi"), "sdk-envelope-v1\n");
    write_text_file(
        &root.join("agent/coder.d/path"),
        &format!("{}\n", root.join("tool").display()),
    );
    write_text_file(
        &root.join("agent/coder.d/mount"),
        &format!(
            "{}\t{}\trw\trbind,nosuid,nodev\n",
            root.display(),
            root.display()
        ),
    );
    let policy_path = root.join("agent/coder.d/policy");
    let mut policy = ok!(fs::read_to_string(&policy_path));
    policy.push_str("allow coder_t tool:long execute\n");
    write_text_file(&policy_path, &policy);
    write_text_file(&root.join("agent/coder.d/tools"), "long\n");
    assert!(fs::create_dir_all(root.join("tool/long.d")).is_ok());
    write_text_file(
        &root.join("tool/long.d/policy"),
        "allow coder_t tool:long execute\n",
    );
    write_text_file(
        &root.join("tool/long"),
        "#!/bin/sh\ntouch \"$CTX_SOURCE/tool-ready\"\nsh -c 'trap \"\" TERM; sleep 2; touch \"$CTX_SOURCE/tool-leak\"' &\nwait\n",
    );
    set_file_mode(&root.join("tool/long"), 0o755);
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let executable = root.join("agent/coder");
    write_text_file(
        &executable,
        "#!/bin/sh\nIFS= read -r envelope\nif [ \"$CTX_AGENT_STEP\" = 0 ]; then printf '{\"type\":\"tool_call\",\"run\":\"%s\",\"id\":\"long-1\",\"name\":\"long\",\"arguments\":{\"args\":[]}}\\n' \"$CTX_RUN_ID\"; else touch \"$CTX_SOURCE/respawned\"; fi\n",
    );
    set_file_mode(&executable, 0o755);
    let ready = root.join("tool-ready");
    let cancel_root = session_root.clone();
    let (mut client, mut socket) = ok!(UnixStream::pair());
    assert!(client
        .write_all(
            b"{\"op\":\"send\",\"id\":\"r1\",\"session\":\"default\",\"input\":\"cancel tool\"}\n"
        )
        .is_ok());
    assert!(client.shutdown(Shutdown::Write).is_ok());
    let cancel = thread::spawn(move || cancel_when_ready(&cancel_root, &ready));
    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        direct_agent_runtime(&root, &view, &session_root, &executable),
    );
    let cancelled = cancel.join();
    assert!(
        matches!(cancelled, Ok(true)),
        "cancel={cancelled:?} outcome={outcome:?} ready={}",
        root.join("tool-ready").exists()
    );
    let outcome = ok!(outcome);
    assert!(!outcome.jsonl().contains("tool_result"));
    assert!(!root.join("respawned").exists());
    thread::sleep(Duration::from_millis(200));
    assert!(!root.join("tool-leak").exists());
    assert_file_text(&session_root.join("default/state"), "cancelled\n");
}

#[test]
fn agent_process_error_is_host_owned() {
    let root = reference_tree("agent-executable-socket-runtime-error-output");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r"#!/bin/sh
IFS= read -r envelope || exit 2
printf 'model unavailable\n' >&2
exit 1
",
    );
    set_file_mode(&agent_executable, 0o755);

    let (mut client, mut socket) = ok!(process_failure_socket());

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        direct_agent_runtime(&root, &view, &session_root, &agent_executable),
    );
    let outcome = ok!(outcome);
    assert!(outcome.jsonl().contains("\"code\":\"EIO\""));
    assert!(outcome.jsonl().contains("agent process failed"));
    assert!(!outcome.jsonl().contains("EHOSTDOWN"));
    assert!(outcome.jsonl().contains("\"status\":\"error\""));

    let response = ok!(socket_response(&mut client));
    assert!(response.contains("agent process failed"));
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

    let (mut client, mut socket) = ok!(process_failure_socket());

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
                control_dir: None,
            },
        },
    );
    let outcome = ok!(outcome);
    assert!(outcome.jsonl().contains("\"code\":\"EIO\""));
    assert!(outcome.jsonl().contains("agent process failed"));
    assert!(outcome.jsonl().contains("\"status\":\"error\""));

    let response = ok!(socket_response(&mut client));
    assert!(response.contains("agent process failed"));
    assert!(!response.contains(r#""message":"EIO""#));
}

fn cancel_when_ready(session_root: &std::path::Path, ready: &std::path::Path) -> bool {
    for _ in 0..100 {
        if ready.exists() {
            return handle_socket_request_frame(
                session_root,
                "/work",
                Some("debug/echo"),
                r#"{"op":"cancel","id":"r1"}"#,
            )
            .is_ok();
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

fn process_failure_socket() -> std::io::Result<(UnixStream, UnixStream)> {
    let (mut client, socket) = UnixStream::pair()?;
    assert!(
        client
            .write_all(
                br#"{"op":"send","id":"msg-1","session":"default","input":"hi"}
"#,
            )
            .is_ok()
    );
    assert!(client.shutdown(Shutdown::Write).is_ok());
    Ok((client, socket))
}

fn socket_response(client: &mut UnixStream) -> std::io::Result<String> {
    let mut buffer = [0_u8; 512];
    client
        .read(&mut buffer)
        .map(|read| String::from_utf8_lossy(buffer.get(..read).unwrap_or_default()).into_owned())
}

use super::*;
