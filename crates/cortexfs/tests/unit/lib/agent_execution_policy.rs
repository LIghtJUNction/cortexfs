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

#[test]
fn agent_executable_socket_runtime_passes_history_messages() {
    let root = reference_tree("agent-executable-socket-runtime-history");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
printf '{"type":"delta","run":"%s","text":"%s"}\n' "$CTX_RUN_ID" "$CTX_AGENT_HISTORY_MESSAGES"
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
            agent_name: "coder",
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Direct,
        },
    );
    let outcome = ok!(outcome);
    assert!(outcome.jsonl().contains("- user: hi"));
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
fn agent_executable_socket_runtime_does_not_inherit_service_secrets() {
    let root = reference_tree("agent-executable-socket-runtime-env-clear");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
if [ -n "$CTX_SECRET_CANARY" ]; then
  printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
  printf '{"type":"delta","run":"%s","text":"leaked:%s"}\n' "$CTX_RUN_ID" "$CTX_SECRET_CANARY"
  printf '{"type":"done","run":"%s","status":"error"}\n' "$CTX_RUN_ID"
  exit 0
fi
printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
printf '{"type":"delta","run":"%s","text":"secret-not-inherited"}\n' "$CTX_RUN_ID"
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
            agent_name: "coder",
            agent_executable: &agent_executable,
            execution: AgentExecutableSocketExecution::Direct,
        },
    );
    let outcome = ok!(outcome);
    assert!(outcome.jsonl().contains("secret-not-inherited"));
    assert!(!outcome.jsonl().contains("leaked:"));
}

#[test]
fn agent_executable_socket_bwrap_args_apply_agent_sandbox() {
    let root = reference_tree("agent-executable-socket-runtime-bwrap-args");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    let mut env = view.env().to_vec();
    env.push((
        "CTX_PROVIDER_SECRET_PATH".to_owned(),
        "/run/user/1000/cortexfs/credentials/coder-default".to_owned(),
    ));
    let runtime = AgentExecutableSocketRuntime {
        ctx_root: &root,
        source_root: &root,
        identity: view.identity(),
        env: &env,
        session_root: &session_root,
        default_cwd: "/workspace",
        model: Some("debug/echo"),
        agent_name: "coder",
        agent_executable: &agent_executable,
        execution: AgentExecutableSocketExecution::Bwrap {
            program: Path::new("/usr/bin/bwrap"),
            mount_table: view.mount_table(),
        },
    };

    let args = agent_executable_socket_bwrap_args(&BwrapAgentExecutableArgs {
        runtime,
        mount_table: view.mount_table(),
        cwd: "/workspace",
        run_id: "run-1",
        session: "default",
        history_messages: "- user: hi",
        debug: None,
        input: "hi",
    });

    assert!(args.contains(&"--clearenv".to_owned()));
    assert!(!args.contains(&"--unshare-net".to_owned()));
    assert!(args.contains(&"--unshare-pid".to_owned()));
    assert!(contains_arg_pair(&args, "--tmpfs", "/tmp"));
    assert!(contains_arg_pair(&args, "--ro-bind", "/usr"));
    assert!(contains_arg_pair(&args, "--dir", "/workspace"));
    assert!(contains_arg_triplet(
        &args,
        "--ro-bind",
        "/run/user/1000/cortexfs/credentials/coder-default",
        "/run/user/1000/cortexfs/credentials/coder-default"
    ));
    assert!(contains_arg_pair(&args, "--chdir", "/workspace"));
    assert!(contains_arg_triplet(&args, "--ro-bind", "/ctx", "/ctx"));
    assert_eq!(
        args.get(args.len().saturating_sub(2)),
        Some(&agent_executable.display().to_string())
    );
    assert_eq!(args.last().map(String::as_str), Some("hi"));
}

#[test]
fn policy_v0_allows_only_exact_rules() {
    let parsed = PolicyV0::parse(
        "\
allow coder_t tool:fs.read execute
allow coder_t model:debug/echo use
allow coder_t shared:project-a read
",
    );
    let policy = ok!(parsed);

    assert!(policy.allows(
        "coder_t",
        PolicyObjectClass::Tool,
        "fs.read",
        PolicyPermission::Execute
    ));
    assert!(policy.allows(
        "coder_t",
        PolicyObjectClass::Model,
        "debug/echo",
        PolicyPermission::Use
    ));
    assert!(!policy.allows(
        "coder_t",
        PolicyObjectClass::Tool,
        "shell.exec",
        PolicyPermission::Execute
    ));
    assert!(!policy.allows(
        "reviewer_t",
        PolicyObjectClass::Tool,
        "fs.read",
        PolicyPermission::Execute
    ));
    assert!(!policy.allows(
        "coder_t",
        PolicyObjectClass::Shared,
        "project-a",
        PolicyPermission::Write
    ));
}

#[test]
fn policy_v0_checks_child_authority_subset() {
    let parent = PolicyV0::parse(
        "\
allow coder_t tool:fs.read execute
allow coder_t model:debug/echo use
allow coder_t shared:project-a read
allow coder_t session:default resume
",
    );
    let parent = ok!(parent);

    let child = PolicyV0::parse(
        "\
allow reviewer_t tool:fs.read execute
allow reviewer_t model:debug/echo use
allow reviewer_t shared:project-a read
",
    );
    let child = ok!(child);
    assert!(child.is_authority_subset_of(&parent, "reviewer_t", "coder_t"));
    assert!(!child.is_exact_subset_of(&parent));

    let expanded_tool = PolicyV0::parse(
        "\
allow reviewer_t tool:shell.exec execute
",
    );
    let expanded_tool = ok!(expanded_tool);
    assert!(!expanded_tool.is_authority_subset_of(&parent, "reviewer_t", "coder_t"));

    let wrong_subject = PolicyV0::parse(
        "\
allow other_t tool:fs.read execute
",
    );
    let wrong_subject = ok!(wrong_subject);
    assert!(!wrong_subject.is_authority_subset_of(&parent, "reviewer_t", "coder_t"));
}

#[test]
fn policy_v0_rejects_invalid_rules() {
    assert_eq!(
        PolicyRule::parse("deny coder_t tool:fs.read execute"),
        Err(PolicyError::ExpectedAllow)
    );
    assert_eq!(
        PolicyRule::parse("allow coder_t provider:openai use"),
        Err(PolicyError::UnknownClass)
    );
    assert_eq!(
        PolicyRule::parse("allow coder_t tool:fs.read use"),
        Err(PolicyError::UnknownPermission)
    );
    assert_eq!(
        PolicyRule::parse("allow coder_t tool:* execute"),
        Err(PolicyError::InvalidName)
    );
    assert_eq!(
        PolicyRule::parse("allow coder_t tool:fs.read execute extra"),
        Err(PolicyError::WrongFieldCount)
    );
}

#[test]
fn mount_table_parses_fixed_v0_format() {
    let parsed = MountTable::parse(
        "\
/ctx\t/ctx\tro\trbind,nosuid,nodev,noexec
/home/me/project\t/work\trw\trbind,nosuid,nodev
/tmp\t/tmp\trw\t-
",
    );
    let table = ok!(parsed);
    assert_eq!(table.entries().len(), 3);

    let Some(first) = table.entries().first() else {
        return;
    };
    assert_eq!(first.source(), "/ctx");
    assert_eq!(first.target(), "/ctx");
    assert_eq!(first.mode(), MountMode::ReadOnly);
    assert_eq!(
        first.options(),
        [
            MountOption::RecursiveBind,
            MountOption::NoSuid,
            MountOption::NoDev,
            MountOption::NoExec
        ]
    );

    let Some(last) = table.entries().last() else {
        return;
    };
    assert!(last.options().is_empty());
}

#[test]
fn mount_table_checks_child_attenuation() {
    let parent = MountTable::parse(
        "\
/home/me/project\t/work\trw\trbind,nosuid,nodev
/ctx/shared/project-a\t/shared/project-a\tro\trbind,nosuid,nodev,noexec
",
    );
    let parent = ok!(parent);

    let narrowed = MountTable::parse(
        "\
/home/me/project\t/work\tro\tbind,nosuid,nodev,noexec
/ctx/shared/project-a\t/shared/project-a\tro\tbind,nosuid,nodev,noexec
",
    );
    let narrowed = ok!(narrowed);
    assert!(narrowed.is_subset_of(&parent));

    let write_expansion = MountTable::parse(
        "\
/ctx/shared/project-a\t/shared/project-a\trw\tbind,nosuid,nodev,noexec
",
    );
    let write_expansion = ok!(write_expansion);
    assert!(!write_expansion.is_subset_of(&parent));

    let removed_safety = MountTable::parse(
        "\
/ctx/shared/project-a\t/shared/project-a\tro\tbind,nosuid,nodev
",
    );
    let removed_safety = ok!(removed_safety);
    assert!(!removed_safety.is_subset_of(&parent));

    let hidden_parent_path = MountTable::parse(
        "\
/secret\t/secret\tro\tbind,nosuid,nodev,noexec
",
    );
    let hidden_parent_path = ok!(hidden_parent_path);
    assert!(!hidden_parent_path.is_subset_of(&parent));
}

#[test]
fn mount_table_rejects_invalid_v0_format() {
    assert_eq!(
        MountEntry::parse("ctx\t/ctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx\tctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx/shared/project-a/..\t/ctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx/./shared/project-a\t/ctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx\0/shared/project-a\t/ctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx\u{1b}/shared/project-a\t/ctx\tro\trbind"),
        Err(MountError::InvalidPath)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tbad\trbind"),
        Err(MountError::InvalidMode)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro\tbind,rbind"),
        Err(MountError::ConflictingBindOption)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro\trbind,rbind"),
        Err(MountError::DuplicateOption)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro\tdev"),
        Err(MountError::InvalidOption)
    );
    assert_eq!(
        MountEntry::parse("/ctx\t/ctx\tro"),
        Err(MountError::WrongFieldCount)
    );
}
