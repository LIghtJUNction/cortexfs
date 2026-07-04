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
            network_allowed: false,
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
        "CTX_PROVIDER_CONFIG_DIR".to_owned(),
        "/host/providers.d".to_owned(),
    ));
    env.push(("CTX_PROVIDER_SECRET_FD".to_owned(), "9".to_owned()));
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
        network_allowed: false,
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
        workspace: Some("/repo"),
        run_id: "run-1",
        session: "default",
        history_messages: "- user: hi",
        debug: None,
        input: "hi",
    });

    assert!(args.contains(&"--clearenv".to_owned()));
    assert!(args.contains(&"--unshare-net".to_owned()));
    assert!(args.contains(&"--unshare-pid".to_owned()));
    assert!(contains_arg_pair(&args, "--tmpfs", "/tmp"));
    assert!(contains_arg_pair(&args, "--ro-bind", "/usr"));
    assert!(contains_arg_pair(&args, "--dir", "/workspace"));
    assert!(contains_arg_triplet(
        &args,
        "--setenv",
        "CTX_AGENT_HISTORY_MESSAGES",
        "- user: hi"
    ));
    assert!(contains_arg_triplet(
        &args,
        "--setenv",
        "CTX_PROVIDER_CONFIG_DIR",
        &root.join("shared/providers.d").display().to_string()
    ));
    assert!(!args.iter().any(|arg| arg == "/host/providers.d"));
    assert!(contains_arg_triplet(
        &args,
        "--ro-bind-data",
        "9",
        "/run/user/1000/cortexfs/credentials/coder-default"
    ));
    assert!(contains_arg_pair(&args, "--chdir", "/workspace"));
    assert!(contains_arg_triplet(&args, "--bind", "/repo", "/workspace"));
    assert!(contains_arg_triplet(
        &args,
        "--ro-bind",
        root.to_str().unwrap_or_default(),
        "/ctx"
    ));
    assert!(contains_arg_triplet(
        &args,
        "--ro-bind",
        root.to_str().unwrap_or_default(),
        root.to_str().unwrap_or_default()
    ));
    assert_eq!(
        args.get(args.len().saturating_sub(2)),
        Some(&agent_executable.display().to_string())
    );
    assert_eq!(args.last().map(String::as_str), Some("hi"));
}

#[test]
fn agent_executable_socket_bwrap_args_preserve_network_when_policy_allows() {
    let root = reference_tree("bwrap-network");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    let runtime = AgentExecutableSocketRuntime {
        ctx_root: &root,
        source_root: &root,
        identity: view.identity(),
        env: view.env(),
        session_root: &session_root,
        default_cwd: "/workspace",
        model: Some("debug/echo"),
        network_allowed: true,
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
        workspace: None,
        run_id: "run-1",
        session: "default",
        history_messages: "- user: hi",
        debug: None,
        input: "hi",
    });

    assert!(!args.contains(&"--unshare-net".to_owned()));
    assert!(args.contains(&"--unshare-pid".to_owned()));
}

#[test]
fn agent_executable_socket_bwrap_args_preserve_explicit_workspace_mount() {
    let root = reference_tree("agent-bwrap-explicit-workspace");
    let session_root = agent_session_root(&root, "coder");
    let control = root.join("agent").join("coder.d");
    write_text_file(
        &control.join("mount"),
        "/ctx\t/ctx\tro\trbind,nosuid,nodev\n/repo-explicit\t/workspace\trw\trbind,nosuid,nodev\n",
    );
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    let runtime = AgentExecutableSocketRuntime {
        ctx_root: &root,
        source_root: &root,
        identity: view.identity(),
        env: view.env(),
        session_root: &session_root,
        default_cwd: "/workspace",
        model: Some("debug/echo"),
        network_allowed: false,
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
        workspace: Some("/repo-default"),
        run_id: "run-1",
        session: "default",
        history_messages: "- user: hi",
        debug: None,
        input: "hi",
    });

    assert!(contains_arg_triplet(
        &args,
        "--bind",
        "/repo-explicit",
        "/workspace"
    ));
    assert!(!contains_arg_triplet(
        &args,
        "--bind",
        "/repo-default",
        "/workspace"
    ));
}
