#[test]
fn parses_spec_which_command() {
    let command = cmd!("which", "tool", "fs.read");
    assert!(matches!(
        command,
        Ok(Command::Which(ObjectClass::Tool, ref name)) if name == "fs.read"
    ));
}

#[test]
fn parses_top_level_file_content_commands() {
    let cat = cmd!("cat", "agent/coder.d/cwd");
    assert!(matches!(
        cat,
        Ok(Command::Cat { ref path }) if path == "agent/coder.d/cwd"
    ));

    let set = cmd!("set", "agent/coder.d/cwd", "/work");
    assert!(matches!(
        set,
        Ok(Command::Set { ref path, ref value }) if path == "agent/coder.d/cwd" && value == "/work"
    ));

    let append = cmd!("append", "agent/coder.d/path", "/ctx/tool");
    assert!(matches!(
        append,
        Ok(Command::Append { ref path, ref value })
            if path == "agent/coder.d/path" && value == "/ctx/tool"
    ));
}

#[test]
fn parses_file_metadata_command() {
    let explicit = cmd!("file", "type", "tool/fs.read");
    assert!(matches!(
        explicit,
        Ok(Command::File(ref args))
            if args.command == FileCommand::Type
                && args.path == "tool/fs.read"
    ));

    let shorthand = cmd!("file", "tool/fs.read");
    assert!(matches!(
        shorthand,
        Ok(Command::File(ref args))
            if args.command == FileCommand::Info
                && args.path == "tool/fs.read"
    ));
}

#[test]
fn parses_ls_path_command() {
    let root = cmd!("ls");
    assert!(matches!(root, Ok(Command::Ls(LsTarget::Root))));

    let home = cmd!("ls", "home");
    assert!(matches!(
        home,
        Ok(Command::Ls(LsTarget::Path(ref path))) if path == "home"
    ));

    let tool = cmd!("ls", "tool");
    assert!(matches!(
        tool,
        Ok(Command::Ls(LsTarget::Path(ref path))) if path == "tool"
    ));
}

#[test]
fn parses_session_file_commands() {
    let history = cmd!("history", "coder");
    assert!(matches!(
        history,
        Ok(Command::History {
            ref agent,
            session: None
        }) if agent == "coder"
    ));

    let output = cmd!("agent", "output", "coder", "--session", "default");
    assert!(matches!(
        output,
        Ok(Command::Agent(AgentArgs::Output {
            ref name,
            session: Some(ref session)
        })) if name == "coder" && session == "default"
    ));

    let resume = cmd!("resume", "coder", "default");
    assert!(matches!(
        resume,
        Ok(Command::Resume {
            ref agent,
            session: Some(ref session)
        }) if agent == "coder" && session == "default"
    ));

    let send = cmd!("send", "coder", "default", "hello");
    assert!(matches!(
        send,
        Ok(Command::Send {
            ref agent,
            ref session,
            ref input
        }) if agent == "coder" && session == "default" && input == "hello"
    ));

    let ping = cmd!("ping", "agent/coder");
    assert!(matches!(
        ping,
        Ok(Command::Ping { ref path }) if path == "agent/coder"
    ));

    let cancel = cmd!("cancel", "agent/coder", "run-1");
    assert!(matches!(
        cancel,
        Ok(Command::Cancel { ref path, ref run }) if path == "agent/coder" && run == "run-1"
    ));
}

#[test]
fn parses_subcommand_help_before_required_args() {
    let latest = cmd!("latest", "--help");
    assert!(matches!(
        latest,
        Err(ref error) if error.code == 2 && error.message == "unknown command: latest"
    ));

    let agent = cmd!("agent", "--help");
    assert!(matches!(
        agent,
        Ok(Command::HelpTopic(ref topic)) if topic == "agent"
    ));

    let agent_watch = cmd!("agent", "watch", "--help");
    assert!(matches!(
        agent_watch,
        Ok(Command::HelpTopic(ref topic)) if topic == "agent watch"
    ));
}


#[test]
fn parses_literal_help_as_positional_argument() {
    let which_tool = cmd!("which-tool", "help");
    assert!(matches!(
        which_tool,
        Ok(Command::Which(ObjectClass::Tool, ref name)) if name == "help"
    ));

    let validate_name = cmd!("validate-name", "help");
    assert!(matches!(
        validate_name,
        Ok(Command::ValidateName(ref name)) if name == "help"
    ));

    let mount = cmd!("mount", "help");
    assert!(matches!(
        mount,
        Ok(Command::Mount { source: None, mountpoint: Some(ref mountpoint) })
            if mountpoint == &PathBuf::from("help")
    ));

    let agent_stop = cmd!("agent", "stop", "help");
    assert!(matches!(
        agent_stop,
        Ok(Command::Agent(AgentArgs::Stop { ref name })) if name == "help"
    ));

    let agent_status = cmd!("agent", "status", "help");
    assert!(matches!(
        agent_status,
        Ok(Command::Agent(AgentArgs::Status { ref name })) if name == "help"
    ));

    let agent_new = cmd!("agent", "new", "help");
    assert!(matches!(
        agent_new,
        Ok(Command::Agent(AgentArgs::New(ref args))) if args.name == "help"
    ));
}

#[test]
fn parses_agent_session_client_commands() {
    let send = cmd!("agent", "send", "coder", "--session", "test", "hello", "world");
    assert!(matches!(
        send,
        Ok(Command::Agent(AgentArgs::Send {
            ref name,
            session: Some(ref session),
            ref input,
            raw: false,
        })) if name == "coder" && session == "test" && input == "hello world"
    ));

    let repl = cmd!("agent", "repl", "coder", "--raw");
    assert!(matches!(
        repl,
        Ok(Command::Agent(AgentArgs::Repl {
            ref name,
            session: None,
            raw: true,
        })) if name == "coder"
    ));

    let cancel = cmd!("agent", "cancel", "coder", "--session", "test", "run-1");
    assert!(matches!(
        cancel,
        Ok(Command::Agent(AgentArgs::Cancel {
            ref name,
            session: Some(ref session),
            run: Some(ref run),
            raw: false,
        })) if name == "coder" && session == "test" && run == "run-1"
    ));
}

#[test]
fn parses_agent_lifecycle_commands() {
    let new = cmd!(
        "agent",
        "new",
        "reviewer",
        "--temp",
        "--label",
        "reviewer_t",
        "--model",
        "openai/gpt-4o",
        "--tool",
        "fs.read",
        "--shared",
        "project-a:read",
        "--mount",
        "/work",
        "/work",
        "ro",
    );
    assert!(matches!(
        new,
        Ok(Command::Agent(AgentArgs::New(ref args)))
            if args.name == "reviewer"
                && args.temporary
                && args.label.as_deref() == Some("reviewer_t")
                && args.models == ["openai/gpt-4o".to_owned()]
                && args.tools == ["fs.read".to_owned()]
                && args.shared.len() == 1
                && args.mounts.len() == 1
    ));

    let start = cmd!(
        "agent",
        "start",
        "reviewer",
        "--session",
        "test",
        "--mount",
        "/repo",
        "/workspace",
        "rw",
        "--cwd",
        "/workspace",
    );
    assert!(matches!(
        start,
        Ok(Command::Agent(AgentArgs::Start(ref args)))
            if args.name == "reviewer"
                && args.session == "test"
                && args.cwd == "/workspace"
                && args.default_workspace
                && args.mounts.len() == 1
    ));

    let stop = cmd!("agent", "stop", "reviewer");
    assert!(matches!(
        stop,
        Ok(Command::Agent(AgentArgs::Stop { ref name })) if name == "reviewer"
    ));

    let status = cmd!("agent", "status", "reviewer");
    assert!(matches!(
        status,
        Ok(Command::Agent(AgentArgs::Status { ref name })) if name == "reviewer"
    ));

    let ps = cmd!("agent", "ps");
    assert!(matches!(ps, Ok(Command::Agent(AgentArgs::Ps))));

    let watch = cmd!("agent", "watch", "coder", "--session", "test");
    assert!(matches!(
        watch,
        Ok(Command::Agent(AgentArgs::Watch {
            ref name,
            session: Some(ref session)
        })) if name == "coder" && session == "test"
    ));

    let attach = cmd!("agent", "attach", "coder");
    assert!(matches!(
        attach,
        Ok(Command::Agent(AgentArgs::Attach {
            ref name,
            session: None
        })) if name == "coder"
    ));
}

#[test]
fn agent_new_request_json_matches_lifecycle_tool_shape() {
    let command = cmd!(
        "agent",
        "new",
        "reviewer",
        "--label",
        "reviewer_t",
        "--model",
        "openai/gpt-4o",
        "--tool",
        "fs.read",
        "--shared",
        "project-a:read",
        "--mount",
        "/work",
        "/work",
        "ro",
    );
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };
    assert_eq!(
        agent_new_request_json(&args),
        Ok(
            "{\"name\":\"reviewer\",\"label\":\"reviewer_t\",\"model\":[\"openai/gpt-4o\"],\"tools\":[\"fs.read\"],\"shared\":{\"project-a\":[\"read\"]},\"mount\":[[\"/work\",\"/work\",\"ro\"]]}".to_owned()
        )
    );
}

#[test]
fn agent_new_temp_request_json_includes_lifecycle() {
    let command = cmd!("agent", "new", "scratch", "--temp");
    assert!(matches!(command, Ok(Command::Agent(AgentArgs::New(_)))));
    let Ok(Command::Agent(AgentArgs::New(args))) = command else {
        return;
    };
    assert_eq!(
        agent_new_request_json(&args),
        Ok("{\"name\":\"scratch\",\"life\":\"temp\"}".to_owned())
    );
}

#[test]
fn agent_ps_reads_parent_status_and_pid_controls() {
    let root = clean_test_dir("ctx-agent-ps");
    create_agent_fixture(&root, "coder", "", "idle", "");
    create_agent_fixture(&root, "reviewer", "agent:coder session:default run:r1", "busy", "123");
    create_agent_fixture(&root, "auditor", "agent:reviewer", "ready", "");

    let processes = read_agent_processes(&root);
    assert!(processes.is_ok());
    let mut processes = processes.unwrap_or_default();
    processes.sort_by(|left, right| left.name.cmp(&right.name));
    assert!(processes.iter().any(|process| {
        process.name == "reviewer"
            && process.parent.as_deref() == Some("coder")
            && process.status == "busy"
            && process.pid.as_deref() == Some("123")
    }));

    let root_process = processes
        .iter()
        .find(|process| process.name == "coder")
        .cloned();
    assert!(root_process.is_some());
    let Some(root_process) = root_process else {
        return;
    };
    let mut rendered = Vec::new();
    render_agent_process_tree(&root_process, &processes, "", true, true, &mut rendered);
    assert_eq!(
        rendered,
        vec![
            "coder [idle]".to_owned(),
            "`- reviewer [busy] pid=123".to_owned(),
            "   `- auditor [ready]".to_owned(),
        ]
    );
}

#[test]
fn status_helpers_report_ctx_and_agent_tree() {
    let root = clean_test_dir("ctx-status-tree");
    write_text_file(&root.join("status"), "ready\n");
    create_agent_fixture(&root, "coder", "", "idle", "");
    create_agent_fixture(&root, "reviewer", "agent:coder session:default run:r1", "busy", "123");

    assert_eq!(ctx_state(true, true, true), "running");
    assert_eq!(ctx_state(true, true, false), "available");
    assert_eq!(read_ctx_status(&root), "ready");

    let processes = read_status_agent_processes(&root);
    assert!(processes.is_ok());
    let rendered = render_agent_status_lines(&processes.unwrap_or_default());
    assert_eq!(
        rendered,
        vec![
            "coder [idle]".to_owned(),
            "`- reviewer [busy] pid=123".to_owned(),
        ]
    );
}

#[test]
fn status_tolerates_missing_agent_directory() {
    let root = clean_test_dir("ctx-status-no-agent");
    write_text_file(&root.join("status"), "ready\n");

    let processes = read_status_agent_processes(&root);
    assert_eq!(processes, Ok(Vec::new()));
}

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
    assert!(contains_arg_triplet(&bwrap, "--ro-bind", "/ctx", "/ctx"));
    assert!(contains_arg_triplet(
        &bwrap,
        "--bind",
        "/ctx/home/1000/agent/coder",
        "/home/agent"
    ));
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
    let cli_mounts = Vec::new();
    let bwrap = agent_bwrap_args(&root, &args, &cli_mounts, &view, &socket, &home);
    assert!(!contains_arg_triplet(
        &bwrap,
        "--ro-bind",
        source.join(".git").to_str().unwrap_or_default(),
        "/workspace/.git"
    ));
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
        command.program == "systemd-run"
                && command.args.contains(&"--user".to_owned())
                && contains_arg_pair(&command.args, "--property", "Restart=always")
                && contains_arg_pair(&command.args, "--property", "RestartSec=250ms")
                && command.args.contains(&"-i".to_owned())
                && command.args.contains(&"PATH=/usr/bin:/bin".to_owned())
                && command.args.contains(&"/usr/bin/bwrap".to_owned())
                && contains_arg_pair(&command.args, "--clearenv", "--setenv")
                && contains_arg_triplet(&command.args, "--setenv", "CTX_ROOT", &root.display().to_string())
                && contains_arg_triplet(&command.args, "--setenv", "CTX_HOME", &root.join("home").join("1000").display().to_string())
                && contains_arg_triplet(&command.args, "--setenv", "CTX_AGENT", "coder")
                && contains_arg_triplet(&command.args, "--setenv", "CTX_AGENT_SUBJECT", "coder_t")
                && contains_arg_triplet(&command.args, "--setenv", "HOME", "/home/agent")
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
fn shell_quote_arg_escapes_single_quotes() {
    assert_eq!(shell_quote_arg("default"), "default");
    assert_eq!(shell_quote_arg("has space"), "'has space'");
    assert_eq!(shell_quote_arg("can't"), "'can'\\''t'");
}

#[test]
fn cli_names_accept_abi_valid_uppercase_names() {
    for name in ["NAME", "SESSION", "AGENT", "SOURCE", "TARGET", "PATH", "INPUT", "RUN"] {
        assert!(require_cli_name("agent name", name).is_ok(), "{name}");
        assert!(require_session_name(name).is_ok(), "{name}");
    }
}

fn contains_arg_pair(args: &[String], first: &str, second: &str) -> bool {
    args.windows(2)
        .any(|window| window.first().map(String::as_str) == Some(first)
            && window.get(1).map(String::as_str) == Some(second))
}

fn contains_arg_triplet(args: &[String], first: &str, second: &str, third: &str) -> bool {
    args.windows(3)
        .any(|window| window.first().map(String::as_str) == Some(first)
            && window.get(1).map(String::as_str) == Some(second)
            && window.get(2).map(String::as_str) == Some(third))
}

fn contains_ro_bind_stub(args: &[String], target: &str) -> bool {
    args.windows(3).any(|window| {
        window.first().map(String::as_str) == Some("--ro-bind")
            && window
                .get(1)
                .is_some_and(|source| source.ends_with("/.empty-shell-startup"))
            && window.get(2).map(String::as_str) == Some(target)
    })
}

fn create_agent_fixture(root: &Path, name: &str, parent: &str, status: &str, pid: &str) {
    let agent = fixture_path(root, &["agent", name]);
    write_text_file(&agent, "#!/bin/sh\nexit 0\n");
    let metadata = fs::metadata(&agent);
    assert!(metadata.is_ok());
    if let Ok(metadata) = metadata {
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o755);
        assert!(fs::set_permissions(&agent, permissions).is_ok());
    }
    let control = fixture_path(root, &["agent", &format!("{name}.d")]);
    write_text_file(&control.join("parent"), &newline_terminated(parent));
    write_text_file(&control.join("status"), &newline_terminated(status));
    write_text_file(&control.join("pid"), &newline_terminated(pid));
}

#[test]
fn parses_bootstrap_and_mount_commands() {
    let bootstrap = cmd!("bootstrap");
    assert!(matches!(bootstrap, Ok(Command::Bootstrap { source: None })));

    let bootstrap_source = cmd!("bootstrap", "/tmp/cortexfs-source");
    assert!(matches!(
        bootstrap_source,
        Ok(Command::Bootstrap {
            source: Some(ref source)
        }) if source == Path::new("/tmp/cortexfs-source")
    ));

    let mount = cmd!(
        "mount",
        "--source",
        "/tmp/cortexfs-source",
        "/tmp/cortexfs-mount"
    );
    assert!(matches!(
        mount,
        Ok(Command::Mount {
            source: Some(ref source),
            mountpoint: Some(ref mountpoint)
        }) if source == Path::new("/tmp/cortexfs-source")
            && mountpoint == Path::new("/tmp/cortexfs-mount")
    ));
}

#[test]
fn parses_exec_command_with_arguments() {
    let command = cmd!("exec", "agent/coder", "fix tests");
    assert!(matches!(
        command,
        Ok(Command::Exec {
            ref path,
            ref args
        }) if path == "agent/coder" && args == &["fix tests".to_owned()]
    ));
}

#[test]
fn parses_tool_command_with_arguments() {
    let command = cmd!("tool", "fs.read", "README.md");
    assert!(matches!(
        command,
        Ok(Command::Tool {
            ref name,
            ref args
        }) if name == "fs.read" && args == &["README.md".to_owned()]
    ));
}

#[test]
fn tool_command_refuses_direct_ctx_path_execution() {
    let root = clean_test_dir("ctx-tool-command-visible");
    let tool = root.join("tool").join("project.echo");
    write_text_file(&tool, "#!/bin/sh\nexit 7\n");
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());

    let result = run_visible_tool(&root, "project.echo", &["hello".to_owned()]);
    assert!(matches!(
        result,
        Err(ref error)
            if error.code == 69
                && error.message.contains("direct CTX_PATH execution bypasses CortexFS tool authorization")
    ));
}

#[test]
fn abi_path_resolution_rejects_escape() {
    let root = Path::new("/ctx");
    assert!(resolve_abi_path(root, "agent/coder.d/cwd").is_ok());
    assert!(resolve_abi_path(root, "../etc/passwd").is_err());
    assert!(resolve_abi_path(root, "/etc/passwd").is_err());
    assert!(resolve_abi_path(root, "/ctx/../etc/passwd").is_err());
    assert_eq!(
        resolve_abi_path(root, "/ctx/agent/coder.d/cwd").map(|path| path.display().to_string()),
        Ok("/ctx/agent/coder.d/cwd".to_owned())
    );
}

#[test]
fn ls_lists_abi_paths_and_keeps_object_filtering() {
    let root = clean_test_dir("ctx-ls-paths");
    assert!(ensure_v1_reference_tree(&root).is_ok());

    let home = list_names(&root, &LsTarget::Path("home".to_owned()));
    assert_eq!(home, Ok(vec!["1000".to_owned()]));

    let root_alias = list_names(&root, &LsTarget::Path("/".to_owned()));
    assert!(matches!(root_alias, Ok(ref names) if names.contains(&"home".to_owned())));

    let absolute_home = root.join("home");
    let absolute_home = absolute_home.display().to_string();
    let home_absolute = list_names(&root, &LsTarget::Path(absolute_home));
    assert_eq!(home_absolute, Ok(vec!["1000".to_owned()]));

    let absolute_escape = root.join("../outside").display().to_string();
    assert!(list_names(&root, &LsTarget::Path(absolute_escape)).is_err());

    let tool = list_names(&root, &LsTarget::Path("tool".to_owned()));
    assert!(matches!(
        tool,
        Ok(ref names)
            if names.contains(&"tsh".to_owned()) && !names.contains(&"tsh.d".to_owned())
    ));
}

#[test]
fn detects_durable_session_instance_paths() {
    assert_path_matches(
        &[
            "home/1000/agent/coder/session/default",
            "shared/im-qq-dev/agent/bot/session/group-456",
            "home/1000/model/openai/gpt-4o.d/session/default",
            "shared/project-a/model/openai/gpt-4o.d/session/default",
        ],
        is_durable_session_instance_path,
        true,
    );
    assert_path_matches(
        &[
            "home/1000/agent/coder/session",
            "home/1000/agent/coder/session/default/messages.jsonl",
            "shared/project-a/model/openai/gpt-4o/session/default",
        ],
        is_durable_session_instance_path,
        false,
    );
}

#[test]
fn detects_session_control_paths() {
    for (path, expected) in [
        (
            "home/1000/agent/coder/session/default/state",
            Some(SessionControlKind::State),
        ),
        (
            "shared/im-qq-dev/agent/bot/session/group-456/cwd",
            Some(SessionControlKind::Cwd),
        ),
        (
            "home/1000/model/openai/gpt-4o.d/session/default/meta.json",
            Some(SessionControlKind::MetaJson),
        ),
        ("home/1000/agent/coder/session/default/messages.jsonl", None),
    ] {
        assert_path_kind!(path, session_control_path_kind, expected);
    }
}

#[test]
fn detects_private_and_shared_context_pack_paths() {
    assert_path_matches(
        &[
            "home/1000/agent/coder/session/default/context/pack.json",
            "shared/im-qq-dev/agent/bot/session/group-456/context/pack.json",
        ],
        is_context_pack_path,
        true,
    );
    assert_path_matches(
        &[
            "home/1000/agent/coder/session/default/context/pack.md",
            "home/1000/agent/bad/name/session/default/context/pack.json",
        ],
        is_context_pack_path,
        false,
    );
}

#[test]
fn detects_private_and_shared_event_stream_paths() {
    assert_path_matches(
        &[
            "home/1000/agent/coder/session/default/events.jsonl",
            "shared/im-qq-dev/agent/bot/session/group-456/events.jsonl",
            "home/1000/model/openai/gpt-4o.d/session/default/events.jsonl",
            "shared/project-a/model/openai/gpt-4o.d/session/default/events.jsonl",
        ],
        is_session_events_path,
        true,
    );
    assert_path_matches(
        &[
            "home/1000/agent/coder/session/default/messages.jsonl",
            "shared/im-qq-dev/agent/bad/name/session/group-456/events.jsonl",
        ],
        is_session_events_path,
        false,
    );
}

#[test]
fn detects_private_and_shared_message_stream_paths() {
    assert_path_matches(
        &[
            "home/1000/agent/coder/session/default/messages.jsonl",
            "shared/im-qq-dev/agent/bot/session/group-456/messages.jsonl",
            "home/1000/model/openai/gpt-4o.d/session/default/messages.jsonl",
        ],
        is_session_messages_path,
        true,
    );
    assert!(!is_session_messages_path(
        "home/1000/agent/coder/session/default/events.jsonl"
    ));
}

#[test]
fn detects_context_jsonl_paths() {
    for (path, expected) in [
        (
            "home/1000/agent/coder/session/default/context/facts.jsonl",
            Some(ContextJsonlKind::Facts),
        ),
        (
            "shared/im-qq-dev/agent/bot/session/group-456/context/decisions.jsonl",
            Some(ContextJsonlKind::Decisions),
        ),
        (
            "home/1000/model/openai/gpt-4o.d/session/default/context/swap/index.jsonl",
            Some(ContextJsonlKind::SwapIndex),
        ),
        (
            "shared/project-a/model/openai/gpt-4o.d/session/default/context/dedup/index.jsonl",
            Some(ContextJsonlKind::DedupIndex),
        ),
        ("home/1000/agent/coder/session/default/context/pack.json", None),
    ] {
        assert_path_kind!(path, context_jsonl_path_kind, expected);
    }
}

#[test]
fn detects_private_and_shared_session_index_paths() {
    for (path, expected) in [
        (
            "home/1000/agent/coder/session/index/list",
            Some(SessionIndexKind::List),
        ),
        (
            "home/1000/agent/coder/session/index/current",
            Some(SessionIndexKind::Current),
        ),
        (
            "shared/im-qq-dev/agent/bot/session/index/by-cwd/hash-1",
            Some(SessionIndexKind::ByCwd),
        ),
        ("home/1000/agent/coder/session/default", None),
        ("home/1000/agent/bad/name/session/index/list", None),
    ] {
        assert_path_kind!(path, session_index_path_kind, expected);
    }
}

#[test]
fn detects_executable_object_paths() {
    for (path, expected) in [
        (
            "model/openai/gpt-4o",
            Some((ObjectClass::Model, "openai/gpt-4o".to_owned())),
        ),
        ("agent/coder", Some((ObjectClass::Agent, "coder".to_owned()))),
        ("tool/fs.read", Some((ObjectClass::Tool, "fs.read".to_owned()))),
        ("tool/fs.read.d/schema", None),
        ("home/1000", None),
    ] {
        assert_path_kind!(path, executable_object_path, expected);
    }
}

#[test]
fn detects_model_capability_paths() {
    assert_path_matches(
        &["model/openai/gpt-4o.d/cap", "model/google/gemini-2.5-pro.d/cap"],
        is_model_capability_path,
        true,
    );
    assert_path_matches(
        &["tool/fs.read.d/cap", "model/openai/gpt-4o/cap", "model/openai/gpt-4o.d/native"],
        is_model_capability_path,
        false,
    );
}

#[test]
fn detects_model_driver_paths() {
    assert_path_matches(
        &["model/openai/gpt-4o.d/driver", "model/anthropic/claude-sonnet-4.d/driver"],
        is_model_driver_path,
        true,
    );
    assert_path_matches(
        &["model/openai/gpt-4o/driver", "model/openai/gpt-4o.d/cap"],
        is_model_driver_path,
        false,
    );
}

#[test]
fn detects_tool_schema_paths() {
    assert_path_matches(
        &["tool/fs.read.d/schema", "tool/mcp.github.search_issues.d/schema"],
        is_tool_schema_path,
        true,
    );
    assert_path_matches(
        &["tool/fs.read/schema", "model/openai/gpt-4o.d/schema", "tool/bad/name.d/schema"],
        is_tool_schema_path,
        false,
    );
}

#[test]
fn detects_shared_tool_schema_paths() {
    assert_path_matches(
        &[
            "shared/project-a/tool/project.test.d/schema",
            "shared/project-a/tool/mcp.github.search_issues.d/schema",
        ],
        is_shared_tool_schema_path,
        true,
    );
    assert_path_matches(
        &[
            "shared/project-a/tool/project.test.d/policy",
            "tool/project.test.d/schema",
            "shared/project-a/tool/bad/name.d/schema",
        ],
        is_shared_tool_schema_path,
        false,
    );
}

#[test]
fn detects_shared_queue_root_paths() {
    assert_path_matches(
        &["shared/project-a/queue", "shared/im-qq-dev/queue"],
        is_shared_queue_root_path,
        true,
    );
    assert_path_matches(
        &["shared/project-a/queue/pending", "shared/project-a/result", "shared/bad/name/queue"],
        is_shared_queue_root_path,
        false,
    );
}

#[test]
fn detects_agent_control_paths_with_fixed_value_syntax() {
    for (path, expected) in [
        ("agent/coder.d/uid", Some(AgentControlKind::Uid)),
        ("agent/coder.d/life", Some(AgentControlKind::Life)),
        ("agent/rev-1.d/parent", Some(AgentControlKind::Parent)),
        ("agent/coder.d/label", None),
        ("model/openai/gpt-4o.d/session", None),
        ("agent/bad/name.d/uid", None),
    ] {
        assert_path_kind!(path, agent_control_path_kind, expected);
    }
}

#[test]
fn ctx_env_quotes_path_export_root_bin() {
    let exports = env_exports(
        Path::new("/tmp/ctx;echo CORTEXFS_CTX_ENV_EVAL_PWNED >/tmp/pwn #"),
        None,
        None,
    );

    assert_eq!(
        exports[3],
        "export PATH='/tmp/ctx;echo CORTEXFS_CTX_ENV_EVAL_PWNED >/tmp/pwn #/bin':$PATH"
    );
}

#[test]
fn ctx_env_preserves_path_expansion_for_safe_root() {
    let exports = env_exports(Path::new("/ctx"), None, None);

    assert_eq!(exports[3], "export PATH=/ctx/bin:$PATH");
}
