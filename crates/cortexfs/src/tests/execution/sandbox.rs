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
        (
            "CTX_PROVIDER_SECRET_VALUE".to_owned(),
            "value-canary".to_owned(),
        ),
        (
            "CTX_PROVIDER_SECRET_PROVIDER".to_owned(),
            "provider-canary".to_owned(),
        ),
        (
            "CTX_PROVIDER_SECRET_SLOT".to_owned(),
            "slot-canary".to_owned(),
        ),
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
        "/host/providers.d".to_owned(),
    ));
    env.push(("CTX_PROVIDER_SECRET_FD".to_owned(), "9".to_owned()));
    env.push((
        "CTX_PROVIDER_SECRET_PATH".to_owned(),
        "/run/user/1000/cortexfs/credentials/coder-default".to_owned(),
    ));
    env.push((
        "CTX_PROVIDER_SECRET_PROVIDER".to_owned(),
        "openai".to_owned(),
    ));
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
            control_dir: None,
        },
    };

    let args = agent_executable_socket_bwrap_args(&BwrapAgentExecutableArgs {
        runtime,
        mount_table: view.mount_table(),
        cwd: "/workspace",
        debug: None,
        input: "hi",
        agent_executable_fd: 9,
        agent_home_source_fd: 10,
        agent_home_sandbox_fd: 11,
        agent_home: session_root.parent().unwrap_or(&session_root),
        control_socket: Some(Path::new("/run/cortexfs/control/source.sock")),
    });

    assert!(!args.contains(&"--clearenv".to_owned()));
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
    assert!(contains_arg_triplet(
        &args,
        "--bind-fd",
        "10",
        &session_root
            .parent()
            .unwrap_or(&session_root)
            .display()
            .to_string()
    ));
    assert!(contains_arg_triplet(
        &args,
        "--bind-fd",
        "11",
        "/home/agent"
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
    assert!(
        !args
            .iter()
            .any(|arg| arg == "/run/user/1000/cortexfs/credentials/coder-default")
    );
    assert!(contains_arg_pair(&args, "--chdir", "/workspace"));
    assert!(!contains_arg_triplet(
        &args,
        "--bind",
        "/repo",
        "/workspace"
    ));
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
            control_dir: None,
        },
    };
    let request = AgentExecutableRunRequest {
        run_id: "run-1",
        cancellation_id: "run-1",
        session: "default",
        cwd: Some("/"),
        input: "hi",
        history_messages: "",
        tool_context: "",
        debug: None,
        envelope: None,
        step: 0,
    };
    let (mut command, agent_executable_fd) =
        agent_executable_socket_command(runtime, &opened, request, None)
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
fn agent_executable_socket_bwrap_preserves_provider_secret_env()
-> Result<(), Box<dyn std::error::Error>> {
    let root = reference_tree("agent-bwrap-provider-secret-env");
    let session_root = agent_session_root(&root, "coder");
    let view = derive_agent_runtime_view(&root, "coder")
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let agent_executable = root.join("agent").join("coder");
    write_text_file(
        &agent_executable,
        "#!/bin/sh\nprintf %s \"$CTX_PROVIDER_SECRET_VALUE\"\n",
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
            control_dir: None,
        },
    };
    let request = AgentExecutableRunRequest {
        run_id: "run-1",
        cancellation_id: "run-1",
        session: "default",
        cwd: Some("/"),
        input: "hi",
        history_messages: "",
        tool_context: "",
        debug: None,
        envelope: None,
        step: 0,
    };

    let (mut command, agent_executable_fd) =
        agent_executable_socket_command(runtime, &opened, request, None)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let output = command.output()?;
    drop(agent_executable_fd);
    assert!(output.status.success(), "bwrap failed: {output:?}");
    assert_eq!(output.stdout, b"provider-secret");
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
                control_dir: None,
            },
        },
    )
    .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    assert!(outcome.jsonl().contains("absent-absent-neutral"));
    assert!(!outcome.jsonl().contains("leaked"));
    Ok(())
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
            control_dir: None,
        },
    };

    let args = agent_executable_socket_bwrap_args(&BwrapAgentExecutableArgs {
        runtime,
        mount_table: view.mount_table(),
        cwd: "/workspace",
        debug: None,
        input: "hi",
        agent_executable_fd: 9,
        agent_home_source_fd: 10,
        agent_home_sandbox_fd: 11,
        agent_home: session_root.parent().unwrap_or(&session_root),
        control_socket: None,
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
            control_dir: None,
        },
    };

    let args = agent_executable_socket_bwrap_args(&BwrapAgentExecutableArgs {
        runtime,
        mount_table: view.mount_table(),
        cwd: "/workspace",
        debug: None,
        input: "hi",
        agent_executable_fd: 9,
        agent_home_source_fd: 10,
        agent_home_sandbox_fd: 11,
        agent_home: session_root.parent().unwrap_or(&session_root),
        control_socket: None,
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
use super::*;

#[test]
#[ignore = "subprocess entrypoint for capability integration test"]
fn capability_subprocess_helper() {
    let result = crate::runtime::control::ping_from_environment("coder");
    assert!(
        result.is_ok(),
        "{result:?} socket={:?} exists={}",
        std::env::var_os("CTX_CONTROL_SOCKET"),
        std::env::var_os("CTX_CONTROL_SOCKET").is_some_and(|path| Path::new(&path).exists())
    );
}

#[test]
fn agent_executable_command_capability_subprocess_roundtrip() {
    let root = reference_tree("agent-capability-subprocess");
    let session_root = agent_session_root(&root, "coder");
    let view = ok!(derive_agent_runtime_view(&root, "coder"));
    let executable = root.join("agent").join("coder");
    let current_exe = ok!(std::env::current_exe());
    write_text_file(
        &executable,
        &format!(
            "#!/bin/sh\nexec '{}' --exact tests::execution::sandbox::capability_subprocess_helper --ignored\n",
            current_exe.display()
        ),
    );
    set_file_mode(&executable, 0o755);
    let control_dir = std::env::temp_dir().join(format!("cfs-cap-{}", std::process::id()));
    assert!(fs::create_dir_all(&control_dir).is_ok());
    assert!(fs::set_permissions(&control_dir, fs::Permissions::from_mode(0o711)).is_ok());
    let (capability, listener) = ok!(crate::runtime::control::RunCapability::create(
        &control_dir,
        "coder",
        "default",
        "msg-1",
        nix::unistd::geteuid().as_raw(),
        nix::unistd::getegid().as_raw(),
    ));
    let environment = capability.environment(capability.socket());
    let capability_socket = capability.socket().to_owned();
    let opened = ok!(open_agent_executable_no_follow(&executable));
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
        agent_executable: &executable,
        execution: AgentExecutableSocketExecution::Direct,
    };
    let request = AgentExecutableRunRequest {
        run_id: "msg-1",
        cancellation_id: "msg-1",
        session: "default",
        cwd: None,
        input: "ignored",
        history_messages: "[]",
        tool_context: "",
        debug: None,
        envelope: None,
        step: 0,
    };
    let (mut command, _fds) = ok!(agent_executable_socket_command(
        runtime,
        &opened,
        request,
        Some((capability.socket(), environment.as_slice())),
    ));
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server_shutdown = std::sync::Arc::clone(&shutdown);
    let (startup_sender, startup_receiver) = std::sync::mpsc::sync_channel(1);
    let server = std::thread::spawn(move || {
        let result = capability.serve_run(&listener, &server_shutdown, &startup_sender, || {
            Some("msg-1".to_owned())
        });
        let cleanup = capability.cleanup();
        result.and(cleanup)
    });
    let output = ok!(command.output());
    assert!(
        output.status.success(),
        "subprocess failed: {} {}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(matches!(startup_receiver.recv(), Ok(Ok(()))));
    shutdown.store(true, std::sync::atomic::Ordering::Release);
    assert!(matches!(server.join(), Ok(Ok(()))));
    assert!(!capability_socket.exists());
    assert!(fs::remove_dir(&control_dir).is_ok());

    let (mut command, _fds) = ok!(agent_executable_socket_command(
        runtime, &opened, request, None
    ));
    assert!(ok!(command.output()).status.success());
}
