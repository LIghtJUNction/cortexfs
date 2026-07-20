#[test]
fn agent_executable_socket_direct_does_not_inherit_provider_secrets() {
    let root = reference_tree("agent-direct-secrets");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
if [ -n "$CTX_PROVIDER_SECRET_VALUE$CTX_PROVIDER_SECRET_PROVIDER$CTX_PROVIDER_SECRET_SLOT$CTX_PROVIDER_SECRET_FD$CTX_PROVIDER_SECRET_PATH" ]; then
  printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
  printf '{"type":"delta","run":"%s","text":"leaked:%s:%s:%s:%s:%s"}\n' "$CTX_RUN_ID" "$CTX_PROVIDER_SECRET_VALUE" "$CTX_PROVIDER_SECRET_PROVIDER" "$CTX_PROVIDER_SECRET_SLOT" "$CTX_PROVIDER_SECRET_FD" "$CTX_PROVIDER_SECRET_PATH"
  printf '{"type":"done","run":"%s","status":"error"}\n' "$CTX_RUN_ID"
  exit 0
fi
printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
printf '{"type":"delta","run":"%s","text":"secret-not-inherited"}\n' "$CTX_RUN_ID"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
"#,
    );
    set_file_mode(&agent_executable, 0o755);
    let mut env = view.env().to_vec();
    env.extend([
        ("CTX_PROVIDER_SECRET_VALUE".to_owned(), "value-canary".to_owned()),
        (
            "CTX_PROVIDER_SECRET_PROVIDER".to_owned(),
            "provider-canary".to_owned(),
        ),
        ("CTX_PROVIDER_SECRET_SLOT".to_owned(), "slot-canary".to_owned()),
        ("CTX_PROVIDER_SECRET_FD".to_owned(), "42".to_owned()),
        (
            "CTX_PROVIDER_SECRET_PATH".to_owned(),
            "/secret/path-canary".to_owned(),
        ),
    ]);

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
            env: &env,
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
    for canary in [
        "value-canary",
        "provider-canary",
        "slot-canary",
        "42",
        "/secret/path-canary",
    ] {
        assert!(!outcome.jsonl().contains(canary));
    }
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
        "/host/providers.d".to_owned()));
    env.push(("CTX_PROVIDER_SECRET_FD".to_owned(), "9".to_owned()));
    env.push((
        "CTX_PROVIDER_SECRET_PATH".to_owned(),
        "/run/user/1000/cortexfs/credentials/coder-default".to_owned()));
    env.push(("CTX_PROVIDER_SECRET_PROVIDER".to_owned(), "openai".to_owned()));
    env.push(("CTX_PROVIDER_SECRET_SLOT".to_owned(), "default".to_owned()));
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
        debug: None,
        input: "hi",
        agent_executable_fd: 9,
    });

    assert_eq!(args.first().map(String::as_str), Some("--clearenv"));
    assert!(args.contains(&"--unshare-net".to_owned()));
    assert!(args.contains(&"--unshare-pid".to_owned()));
    assert!(contains_arg_pair(&args, "--tmpfs", "/tmp"));
    assert!(contains_arg_pair(&args, "--ro-bind", "/usr"));
    assert!(contains_arg_pair(&args, "--dir", "/workspace"));
    assert!(contains_arg_pair(&args, "--perms", "0755"));
    assert!(contains_arg_triplet(
        &args,
        "--ro-bind-data",
        "9",
        "/run/cortexfs/agent-executable"
    ));
    assert!(!args.iter().any(|arg| arg == "- user: hi"));
    assert!(!args.iter().any(|arg| arg == "workspace context"));
    assert!(contains_arg_triplet(
        &args,
        "--setenv",
        "CTX_PROVIDER_CONFIG_DIR",
        &root.join("shared/providers.d").display().to_string()
    ));
    assert!(!args.iter().any(|arg| arg == "/host/providers.d"));
    assert!(!args.iter().any(|arg| arg == "CTX_PROVIDER_SECRET_FD"));
    assert!(!args.iter().any(|arg| arg == "CTX_PROVIDER_SECRET_PATH"));
    assert!(!args.iter().any(|arg| arg == "CTX_PROVIDER_SECRET_PROVIDER"));
    assert!(!args.iter().any(|arg| arg == "CTX_PROVIDER_SECRET_SLOT"));
    assert!(!args
        .iter()
        .any(|arg| arg == "/run/user/1000/cortexfs/credentials/coder-default"));
    assert!(contains_arg_pair(&args, "--chdir", "/workspace"));
    assert!(!contains_arg_triplet(&args, "--bind", "/repo", "/workspace"));
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
        Some(&"/run/cortexfs/agent-executable".to_owned())
    );
    assert_eq!(args.last().map(String::as_str), Some("hi"));

    let opened = ok!(open_agent_executable_no_follow(&agent_executable));
    let request = AgentExecutableRunRequest {
        run_id: "run-1",
        session: "default",
        cwd: Some("/workspace"),
        input: "hi",
        history_messages: "",
        tool_context: "",
        debug: None,
    };
    let (command, agent_executable_fd) = ok!(agent_executable_socket_command(
        runtime, &opened, request
    ));
    drop(agent_executable_fd);
    let command_env: Vec<_> = command
        .get_envs()
        .filter_map(|(name, _value)| name.to_str())
        .collect();
    for secret_name in [
        "CTX_PROVIDER_SECRET_FD",
        "CTX_PROVIDER_SECRET_PATH",
        "CTX_PROVIDER_SECRET_PROVIDER",
        "CTX_PROVIDER_SECRET_SLOT",
    ] {
        assert!(!command_env.contains(&secret_name));
    }
}

#[test]
fn agent_executable_socket_bwrap_executes_opened_inode_after_path_replacement()
-> Result<(), Box<dyn std::error::Error>> {
    let root = reference_tree("agent-bwrap-opened-inode");
    let session_root = agent_session_root(&root, "coder");
    let view = derive_agent_runtime_view(&root, "coder")
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let agent_executable = root.join("agent").join("coder");
    write_text_file(&agent_executable, "#!/bin/sh\nprintf A\n");
    set_file_mode(&agent_executable, 0o755);
    let opened = open_agent_executable_no_follow(&agent_executable)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let runtime = AgentExecutableSocketRuntime {
        ctx_root: &root,
        source_root: &root,
        identity: view.identity(),
        env: view.env(),
        session_root: &session_root,
        default_cwd: "/",
        model: Some("debug/echo"),
        network_allowed: false,
        agent_name: "coder",
        agent_executable: &agent_executable,
        execution: AgentExecutableSocketExecution::Bwrap {
            program: Path::new("/usr/bin/bwrap"),
            mount_table: view.mount_table(),
        },
    };
    let request = AgentExecutableRunRequest {
        run_id: "run-1",
        session: "default",
        cwd: Some("/"),
        input: "hi",
        history_messages: "",
        tool_context: "",
        debug: None,
    };
    let (mut command, agent_executable_fd) =
        agent_executable_socket_command(runtime, &opened, request)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let replacement = root.join("agent").join("replacement");
    write_text_file(&replacement, "#!/bin/sh\nprintf B\n");
    set_file_mode(&replacement, 0o755);
    fs::rename(replacement, &agent_executable)?;

    let output = command.output()?;
    drop(agent_executable_fd);
    assert!(output.status.success(), "bwrap failed: {output:?}");
    assert_eq!(output.stdout, b"A");
    Ok(())
}

#[test]
fn agent_executable_socket_bwrap_does_not_inherit_provider_secret_env()
-> Result<(), Box<dyn std::error::Error>> {
    let root = reference_tree("agent-bwrap-provider-secret-env");
    let session_root = agent_session_root(&root, "coder");
    let view = derive_agent_runtime_view(&root, "coder")
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        "#!/bin/sh\nprintf %s \"${CTX_PROVIDER_SECRET_VALUE:-secret-not-inherited}\"\n",
    );
    set_file_mode(&agent_executable, 0o755);
    let opened = open_agent_executable_no_follow(&agent_executable)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let mut env = view.env().to_vec();
    env.push((
        "CTX_PROVIDER_SECRET_VALUE".to_owned(),
        "provider-secret".to_owned(),
    ));
    let runtime = AgentExecutableSocketRuntime {
        ctx_root: &root,
        source_root: &root,
        identity: view.identity(),
        env: &env,
        session_root: &session_root,
        default_cwd: "/",
        model: Some("debug/echo"),
        network_allowed: false,
        agent_name: "coder",
        agent_executable: &agent_executable,
        execution: AgentExecutableSocketExecution::Bwrap {
            program: Path::new("/usr/bin/bwrap"),
            mount_table: view.mount_table(),
        },
    };
    let request = AgentExecutableRunRequest {
        run_id: "run-1",
        session: "default",
        cwd: Some("/"),
        input: "hi",
        history_messages: "",
        tool_context: "",
        debug: None,
    };

    let (mut command, agent_executable_fd) =
        agent_executable_socket_command(runtime, &opened, request)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let output = command.output()?;
    drop(agent_executable_fd);
    assert!(output.status.success(), "bwrap failed: {output:?}");
    assert_eq!(output.stdout, b"secret-not-inherited");
    Ok(())
}

#[test]
fn agent_executable_socket_bwrap_ignores_request_workspace()
-> Result<(), Box<dyn std::error::Error>> {
    let root = reference_tree("agent-bwrap-workspace");
    let session_root = agent_session_root(&root, "coder");
    let view = derive_agent_runtime_view(&root, "coder")
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        r#"#!/bin/sh
workspace_env=absent
workspace_mount=absent
workspace_context=neutral
if [ -n "${CTX_WORKSPACE+x}" ]; then workspace_env=leaked; fi
if [ -e /workspace/etc/passwd ]; then workspace_mount=leaked; fi
case "$CTX_AGENT_TOOL_CONTEXT" in
  *'Host workspace configuration: determined by agent policy'*) ;;
  *) workspace_context=leaked ;;
esac
printf '{"type":"start","run":"%s","agent":"coder"}\n' "$CTX_RUN_ID"
printf '{"type":"delta","run":"%s","text":"%s-%s-%s"}\n' "$CTX_RUN_ID" "$workspace_env" "$workspace_mount" "$workspace_context"
printf '{"type":"done","run":"%s","status":"ok"}\n' "$CTX_RUN_ID"
"#,
    );
    set_file_mode(&agent_executable, 0o755);
    let (mut client, mut socket) = UnixStream::pair()?;
    client.write_all(
        br#"{"op":"send","id":"msg-1","session":"default","cwd":"/workspace","workspace":"/","input":"hi"}
"#,
    )?;
    client.shutdown(Shutdown::Write)?;

    let outcome = serve_agent_executable_socket_stream_once(
        &mut socket,
        None,
        AgentExecutableSocketRuntime {
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
        },
    )
    .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    assert!(outcome.jsonl().contains("absent-absent-neutral"));
    assert!(!outcome.jsonl().contains("leaked"));
    Ok(())
}

#[test]
fn agent_executable_socket_bwrap_args_keep_network_namespace_isolated_even_when_policy_allows() {
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
        debug: None,
        input: "hi",
        agent_executable_fd: 9,
    });

    assert!(args.contains(&"--unshare-net".to_owned()));
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
        debug: None,
        input: "hi",
        agent_executable_fd: 9,
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
