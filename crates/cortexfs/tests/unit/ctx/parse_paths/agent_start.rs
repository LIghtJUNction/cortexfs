fn current_uid_for_test() -> String {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|uid| uid.trim().to_owned())
        .filter(|uid| !uid.is_empty())
        .unwrap_or_else(|| "1000".to_owned())
}

#[test]
fn agent_terminal_socket_uses_session_terminal_main_socket() {
    let root = clean_test_dir("ctx-agent-terminal-socket");
    let socket = agent_terminal_socket(&root, "coder", "test");
    assert_eq!(
        socket,
        Ok(root
            .join("home")
            .join(current_uid_for_test())
            .join("agent")
            .join("coder")
            .join("session")
            .join("test")
            .join("terminal")
            .join("main.sock"))
    );
}

#[test]
fn agent_start_builds_sandboxed_terminal_command() {
    let root = clean_test_dir("ctx-agent-start-bwrap-view");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    write_text_file(
        &root.join("agent").join("coder.d").join("env"),
        "CTX_ROOT=/bad\nCTX_PROVIDER_CONFIG_DIR=/bad/providers.d\n",
    );
    let view = derive_agent_runtime_view(&root, "coder");
    assert!(view.is_ok(), "reference coder view: {view:?}");
    let Ok(view) = view else {
        return;
    };
    let args = AgentStartArgs {
        name: "coder".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: vec![AgentMount {
            source: "/repo".to_owned(),
            target: "/workspace".to_owned(),
            mode: "rw".to_owned(),
        }],
    };
    let socket = PathBuf::from("/ctx/home/1000/agent/coder/session/test/terminal/main.sock");
    let home = PathBuf::from("/ctx/home/1000");
    let cli_mounts = vec![AgentMount {
        source: "/repo".to_owned(),
        target: "/workspace".to_owned(),
        mode: "rw".to_owned(),
    }];
    let bwrap = agent_bwrap_args(&root, &args, &cli_mounts, &view, &socket, &home);
    assert!(contains_arg_triplet(
        &bwrap,
        "--ro-bind",
        &root.display().to_string(),
        "/ctx"
    ));
    assert!(contains_arg_triplet(
        &bwrap,
        "--bind",
        &root
            .join("home")
            .join("1000")
            .join("agent")
            .join("coder")
            .display()
            .to_string(),
        "/home/agent"
    ));
    assert!(contains_arg_triplet(&bwrap, "--setenv", "CTX_AGENT_ROLE", "agent"));
    assert!(contains_arg_triplet(
        &bwrap,
        "--setenv",
        "CTX_PROVIDER_CONFIG_DIR",
        "/ctx/shared/providers.d"
    ));
    assert!(!bwrap.iter().any(|arg| arg == "/bad/providers.d"));
    assert!(contains_arg_triplet(&bwrap, "--setenv", "CTX_AGENT_MODEL", "main"));
    assert!(contains_arg_triplet(&bwrap, "--setenv", "CTX_AGENT_LIFE", "owned"));
    assert!(contains_arg_triplet(&bwrap, "--bind", "/repo", "/workspace"));
    assert!(bwrap.contains(&"--unshare-net".to_owned()));
    assert!(contains_arg_pair(&bwrap, "--dir", "/home"));
    assert!(contains_ro_bind_stub(&bwrap, "/etc/profile"));
    assert!(contains_ro_bind_stub(&bwrap, "/etc/bash.bashrc"));
    assert!(contains_arg_pair(&bwrap, "--tmpfs", "/etc/profile.d"));
    assert!(contains_arg_pair(&bwrap, "--chdir", "/workspace"));
    assert!(contains_arg_pair(&bwrap, "--listen", socket.to_str().unwrap_or_default()));
    assert_eq!(bwrap.last().map(String::as_str), Some("/ctx/bin/tsh"));
}

#[test]
fn agent_start_default_workspace_remounts_git_read_only() {
    let source = clean_test_dir("ctx-agent-start-git-ro");
    assert!(fs::create_dir_all(source.join(".git")).is_ok());
    let args = AgentStartArgs {
        name: "coder".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };

    let mounts = agent_start_mounts_with_default_source(&args, &source);
    assert_eq!(
        mounts,
        vec![
            AgentMount {
                source: source.display().to_string(),
                target: "/workspace".to_owned(),
                mode: "rw".to_owned(),
            },
            AgentMount {
                source: source.join(".git").display().to_string(),
                target: "/workspace/.git".to_owned(),
                mode: "ro".to_owned(),
            },
        ]
    );

    let root = clean_test_dir("ctx-agent-start-git-bwrap-view");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let view = derive_agent_runtime_view(&root, "coder");
    assert!(view.is_ok(), "reference coder view: {view:?}");
    let Ok(view) = view else {
        return;
    };
    let socket = PathBuf::from("/ctx/home/1000/agent/coder/session/test/terminal/main.sock");
    let home = PathBuf::from("/ctx/home/1000");
    let cli_mounts = mounts;
    let bwrap = agent_bwrap_args(&root, &args, &cli_mounts, &view, &socket, &home);
    assert!(contains_arg_triplet(
        &bwrap,
        "--ro-bind",
        source.join(".git").to_str().unwrap_or_default(),
        "/workspace/.git"
    ));
}

#[test]
fn agent_start_maps_ctx_mount_sources_to_selected_root() {
    let root = clean_test_dir("ctx-agent-start-alt-root-mount-source");

    assert_eq!(agent_host_mount_source(&root, "/ctx"), root.display().to_string());
    assert_eq!(
        agent_host_mount_source(&root, "/ctx/home/1000/agent/coder"),
        root.join("home")
            .join("1000")
            .join("agent")
            .join("coder")
            .display()
            .to_string()
    );
    assert_eq!(
        agent_host_mount_source(&root, "/host/input"),
        "/host/input".to_owned()
    );
}

#[test]
fn agent_start_maps_host_cwd_to_sandbox_mount_target() {
    let source = clean_test_dir("ctx-agent-start-host-cwd");
    let subdir = source.join("nested");
    assert!(fs::create_dir_all(&subdir).is_ok());
    let args = AgentStartArgs {
        name: "coder".to_owned(),
        session: "test".to_owned(),
        cwd: subdir.display().to_string(),
        default_workspace: true,
        mounts: Vec::new(),
    };
    let mounts = agent_start_mounts_with_default_source(&args, &source);

    assert_eq!(agent_start_sandbox_cwd(&args, &mounts), "/workspace/nested");
}

#[test]
fn agent_start_records_ready_status_and_start_event() {
    let root = clean_test_dir("ctx-agent-start-record-state");
    let control = root.join("agent/scratch.d");
    create_agent_fixture(&root, "scratch", "agent:base", "start", "");
    write_text_file(&control.join("log"), "");
    let args = AgentStartArgs {
        name: "scratch".to_owned(),
        session: "default".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };

    let facts = [("model", "main"), ("life", "owned"), ("role", "agent"), ("uid", "1000"), ("gid", "100"), ("groups", "10 20")];
    assert_eq!(
        record_agent_start_state(&root, &args, "cortexfs-agent-scratch-default", &facts, Some("abc123")),
        Ok(())
    );
    assert_eq!(fs::read_to_string(control.join("status")).unwrap_or_default(), "ready\n");
    assert_eq!(fs::read_to_string(control.join("pid")).unwrap_or_default(), "\n");
    assert_eq!(
        fs::read_to_string(control.join("log")).unwrap_or_default(),
        "{\"type\":\"agent.start\",\"agent\":\"scratch\",\"session\":\"default\",\"unit\":\"cortexfs-agent-scratch-default\",\"model\":\"main\",\"life\":\"owned\",\"role\":\"agent\",\"uid\":\"1000\",\"gid\":\"100\",\"groups\":\"10 20\",\"status\":\"ready\",\"invocation\":\"abc123\"}\n"
    );
}

#[test]
fn systemctl_main_pid_parser_ignores_missing_pid() {
    assert_eq!(parse_systemctl_main_pid("0\n"), None);
    assert_eq!(parse_systemctl_main_pid("12345\n"), Some("12345".to_owned()));
}

#[test]
fn agent_start_default_workspace_does_not_remount_symlinked_git() {
    let source = clean_test_dir("ctx-agent-start-git-symlink");
    let target = clean_test_dir("ctx-agent-start-git-symlink-target");
    assert!(fs::create_dir_all(&source).is_ok());
    assert!(fs::create_dir_all(&target).is_ok());
    assert!(std::os::unix::fs::symlink(&target, source.join(".git")).is_ok());
    let args = AgentStartArgs {
        name: "coder".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };

    let mounts = agent_start_mounts_with_default_source(&args, &source);
    assert_eq!(
        mounts,
        vec![AgentMount {
            source: source.display().to_string(),
            target: "/workspace".to_owned(),
            mode: "rw".to_owned(),
        }]
    );
}

#[test]
fn agent_mount_validation_rejects_protected_sandbox_targets() {
    for target in [
        "/",
        "/usr",
        "/usr/local",
        "/etc",
        "/bin",
        "/lib",
        "/lib64",
        "/run",
        "/home",
        "/dev",
        "/proc",
        "/ctx",
        "/ctx/bin",
        "/usr/../ctx",
        "/workspace/../usr/bin",
    ] {
        let mount = AgentMount {
            source: "/tmp/source".to_owned(),
            target: target.to_owned(),
            mode: "rw".to_owned(),
        };
        assert!(
            require_agent_mount(&mount).is_err(),
            "target should be rejected: {target}"
        );
    }
}

#[test]
fn agent_mount_validation_allows_workspace_subtrees() {
    let mount = AgentMount {
        source: "/tmp/source".to_owned(),
        target: "/workspace/project".to_owned(),
        mode: "rw".to_owned(),
    };

    assert!(require_agent_mount(&mount).is_ok());
}

#[test]
fn agent_start_no_default_workspace_does_not_guess_git_mount() {
    let source = clean_test_dir("ctx-agent-start-no-default-git");
    assert!(fs::create_dir_all(source.join(".git")).is_ok());
    let args = AgentStartArgs {
        name: "coder".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: false,
        mounts: Vec::new(),
    };

    let mounts = agent_start_mounts_with_default_source(&args, &source);
    assert!(mounts.is_empty());
}

#[test]
fn agent_start_systemd_command_uses_sanitized_environment() {
    let root = clean_test_dir("ctx-agent-start-systemd-view");
    assert!(ensure_v1_reference_tree(&root).is_ok());
    let view = derive_agent_runtime_view(&root, "coder");
    assert!(view.is_ok(), "reference coder view: {view:?}");
    let Ok(view) = view else {
        return;
    };
    let args = AgentStartArgs {
        name: "coder".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };
    let socket = PathBuf::from("/ctx/home/1000/agent/coder/session/test/terminal/main.sock");
    let cli_mounts = agent_start_mounts_with_default_source(&args, Path::new("/repo"));
    let command = agent_start_systemd_command(
        &root,
        &args,
        &cli_mounts,
        &view,
        &socket,
        "cortexfs-agent-coder-test-terminal",
    );
    assert!(
        command.program == "/usr/bin/systemd-run"
                && command.args.contains(&"--user".to_owned())
                && contains_arg_pair(&command.args, "--property", "Restart=always")
                && contains_arg_pair(&command.args, "--property", "RestartSec=250ms")
                && command.args.contains(&"-i".to_owned())
                && command.args.contains(&"PATH=/usr/bin:/bin".to_owned())
                && command.args.contains(&"/usr/bin/bwrap".to_owned())
                && contains_arg_pair(&command.args, "--clearenv", "--setenv")
                && contains_arg_triplet(&command.args, "--setenv", "CTX_ROOT", "/ctx")
                && contains_arg_triplet(&command.args, "--setenv", "CTX_HOME", "/ctx/home/1000")
                && contains_arg_triplet(&command.args, "--setenv", "CTX_AGENT", "coder")
                && contains_arg_triplet(&command.args, "--setenv", "CTX_AGENT_ROLE", "agent")
                && contains_arg_triplet(&command.args, "--setenv", "CTX_AGENT_MODEL", "main")
                && contains_arg_triplet(&command.args, "--setenv", "CTX_AGENT_LIFE", "owned")
                && contains_arg_triplet(&command.args, "--setenv", "CTX_AGENT_SUBJECT", "coder_t")
                && contains_arg_triplet(&command.args, "--setenv", "HOME", "/home/agent")
                && contains_arg_triplet(&command.args, "--setenv", "PATH", "/usr/bin:/bin")
                && contains_arg_triplet(&command.args, "--setenv", "USER", "coder")
                && contains_arg_triplet(&command.args, "--setenv", "LOGNAME", "coder")
                && contains_arg_triplet(&command.args, "--setenv", "SHELL", "/usr/bin/bash")
                && contains_arg_triplet(&command.args, "--setenv", "TERM", "xterm-256color")
                && contains_arg_triplet(&command.args, "--setenv", "LANG", "C.UTF-8")
                && contains_arg_triplet(&command.args, "--setenv", "CTX_PATH", "/ctx/tool:/ctx/home/1000/tool")
    );
    let bwrap_index = command.args.iter().position(|arg| arg == "/usr/bin/bwrap");
    assert!(bwrap_index.is_some(), "missing bwrap in command: {command:?}");
    let Some(bwrap_index) = bwrap_index else {
        return;
    };
    let bwrap_tail = command.args.get(bwrap_index + 1..).unwrap_or_default();
    assert!(
        !bwrap_tail
            .iter()
            .any(|arg| arg.starts_with("CTX_") && arg.contains('=')),
        "bwrap arguments must not contain raw KEY=value env entries: {command:?}"
    );
}

#[test]
fn agent_start_process_command_uses_clean_runtime_environment() {
    let command = AgentStartCommand {
        program: "/usr/bin/systemd-run".to_owned(),
        args: vec!["--user".to_owned(), "/usr/bin/env".to_owned()],
    };
    let process = agent_start_process_command(&command);
    let mut envs = process
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect::<Vec<_>>();
    envs.sort();

    assert_clean_user_systemd_env(&envs);
}

#[test]
fn systemctl_user_command_uses_clean_runtime_environment() {
    let command = systemctl_user_command(["stop", "cortexfs-agent-coder-test-terminal.service"]);
    let args = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    let mut envs = command
        .get_envs()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
        })
        .collect::<Vec<_>>();
    envs.sort();

    assert_eq!(command.get_program(), "/usr/bin/systemctl");
    assert_eq!(
        args,
        vec![
            "--user".to_owned(),
            "stop".to_owned(),
            "cortexfs-agent-coder-test-terminal.service".to_owned()
        ]
    );
    assert_clean_user_systemd_env(&envs);
}

fn assert_clean_user_systemd_env(envs: &[(String, Option<String>)]) {
    assert!(
        envs.iter()
            .any(|entry| entry.0 == "PATH" && entry.1.as_deref() == Some("/usr/bin:/bin")),
        "missing sanitized PATH in {envs:?}"
    );
    assert!(
        envs.iter().all(|entry| matches!(
            entry.0.as_str(),
            "PATH" | "XDG_RUNTIME_DIR" | "DBUS_SESSION_BUS_ADDRESS"
        )),
        "unexpected systemd client environment in {envs:?}"
    );
}

#[test]
fn agent_start_status_lines_follow_systemctl_shape() {
    let lines = agent_start_status_lines(
        false,
        "coder",
        "main",
        "owned",
        "agent",
        &[("UID", "1000"), ("GID", "100"), ("Groups", "10 20")],
        "default",
        "cortexfs-agent-coder-default-terminal",
        Some("abc123"),
        "/workspace",
        Path::new("/ctx/home/1000/agent/coder/session/default/terminal/main.sock"),
        Path::new("/run/user/1000/cortexfs/terminal/coder/default/main.sock"),
        "1000",
    );

    let expected = [
        "● cortexfs-agent-coder-default-terminal.service - CortexFS agent terminal",
        "     Loaded: loaded (/run/user/1000/systemd/transient/cortexfs-agent-coder-default-terminal.service; transient)",
        "     Active: active (running)", " Invocation: abc123", "      Agent: coder",
        "      Model: main", "       Life: owned", "       Role: agent", "     UID: 1000",
        "     GID: 100", "     Groups: 10 20", "    Session: default", "        CWD: /workspace",
        "     Socket: /ctx/home/1000/agent/coder/session/default/terminal/main.sock",
        " Runtime Socket: /run/user/1000/cortexfs/terminal/coder/default/main.sock",
    ];
    assert_eq!(lines, expected.map(str::to_owned));
}

#[test]
fn visible_terminal_socket_treats_readonly_fuse_errors_as_best_effort() {
    assert!(visible_terminal_write_error_is_best_effort(
        &std::io::Error::from_raw_os_error(nix::libc::ENOSYS)
    ));
    assert!(visible_terminal_write_error_is_best_effort(
        &std::io::Error::from_raw_os_error(nix::libc::EROFS)
    ));
    assert!(visible_terminal_errno_is_best_effort(nix::errno::Errno::ENOSYS));
    assert!(visible_terminal_errno_is_best_effort(nix::errno::Errno::EROFS));
}

#[test]
fn agent_attach_missing_terminal_suggests_start_command() {
    let socket = unique_test_dir("agent-attach-missing-terminal").join("main.sock");
    let result = stream_terminal_socket(&socket, true, "coder", "test");
    assert!(matches!(
        result,
        Err(ref error)
            if error.message.contains("terminal is not running")
                && error.message.contains("ctx agent start coder --session test")
    ));
}

#[test]
fn agent_attach_missing_terminal_quotes_unsafe_session_in_start_hint() {
    let socket = unique_test_dir("agent-attach-missing-terminal-unsafe-session").join("main.sock");
    let result = stream_terminal_socket(&socket, true, "coder", "safe; touch CORTEXFS_HINT_PWNED #");
    assert!(matches!(
        result,
        Err(ref error)
            if error.message.contains("terminal is not running")
                && error.message.contains(
                    "ctx agent start coder --session 'safe; touch CORTEXFS_HINT_PWNED #'"
            )
    ));
}

#[test]
fn agent_start_chat_socket_command_uses_socket_activation() {
    let root = clean_test_dir("ctx-agent-start-chat-socket-command");
    let socket = root.join("runtime").join("coder.sock");
    let unit = agent_chat_unit(&root, "coder");
    let command = agent_chat_socket_systemd_command(&root, "coder", &socket, &unit);

    assert_eq!(command.program, "/usr/bin/systemd-run");
    assert!(command.args.contains(&"--user".to_owned()));
    assert!(contains_arg_pair(&command.args, "--unit", &unit));
    assert!(command.args.contains(&"--collect".to_owned()));
    assert!(contains_arg_pair(
        &command.args,
        "--socket-property",
        &format!("ListenStream={}", socket.display())
    ));
    assert!(contains_arg_pair(
        &command.args,
        "--socket-property",
        "SocketMode=0666"
    ));
    assert!(contains_arg_pair(&command.args, "--agent", "coder"));
    assert!(command
        .args
        .iter()
        .any(|arg| arg.ends_with("cortexfs-agent-runtime")));
}

#[test]
fn agent_start_chat_socket_path_is_root_scoped() {
    let left = clean_test_dir("ctx-agent-chat-socket-left");
    let right = clean_test_dir("ctx-agent-chat-socket-right");

    let left_socket = agent_chat_runtime_socket(&left, "coder");
    let right_socket = agent_chat_runtime_socket(&right, "coder");

    assert!(matches!(left_socket, Ok(ref socket) if socket.ends_with("coder.sock")));
    assert!(matches!((&left_socket, &right_socket), (Ok(left), Ok(right)) if left != right));
    assert_eq!(agent_chat_unit(&left, "coder"), agent_chat_unit(&left, "coder"));
    assert_ne!(agent_chat_unit(&left, "coder"), agent_chat_unit(&right, "coder"));
}
