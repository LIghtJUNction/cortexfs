#[test]
fn parses_spec_which_command() {
    let command = cmd!("which", "tool", "fs.read");
    assert!(matches!(
        command,
        Ok(Command::Which(ObjectClass::Tool, ref name)) if name == "fs.read"
    ));
}

#[test]
fn parses_file_set_command() {
    let command = cmd!("file", "set", "agent/coder.d/cwd", "/work");
    assert!(matches!(
        command,
        Ok(Command::File(ref args))
            if args.command == FileCommand::Set
                && args.path == "agent/coder.d/cwd"
                && args.value.as_deref() == Some("/work")
    ));
}

#[test]
fn parses_file_classify_command() {
    let explicit = cmd!("file", "classify", "tool/fs.read");
    assert!(matches!(
        explicit,
        Ok(Command::File(ref args))
            if args.command == FileCommand::Classify
                && args.path == "tool/fs.read"
                && args.value.is_none()
    ));

    let shorthand = cmd!("file", "tool/fs.read");
    assert!(matches!(
        shorthand,
        Ok(Command::File(ref args))
            if args.command == FileCommand::Classify
                && args.path == "tool/fs.read"
                && args.value.is_none()
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

    let latest = cmd!("latest", "coder", "default");
    assert!(matches!(
        latest,
        Ok(Command::Latest {
            ref agent,
            session: Some(ref session)
        }) if agent == "coder" && session == "default"
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
        Ok(Command::HelpTopic(ref topic)) if topic == "latest"
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
            ref session
        })) if name == "coder" && session == "test"
    ));

    let attach = cmd!("agent", "attach", "coder");
    assert!(matches!(
        attach,
        Ok(Command::Agent(AgentArgs::Attach {
            ref name,
            ref session
        })) if name == "coder" && session == "default"
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
fn agent_terminal_socket_uses_session_terminal_main_socket() {
    let root = clean_test_dir("ctx-agent-terminal-socket");
    let socket = agent_terminal_socket(&root, "coder", "test");
    assert_eq!(
        socket,
        Ok(root
            .join("home")
            .join("1000")
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
    let root = PathBuf::from("/ctx");
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
    let bwrap = agent_bwrap_args(&root, &args, &args.mounts, &socket);
    assert!(contains_arg_triplet(&bwrap, "--bind", "/ctx", "/ctx"));
    assert!(contains_arg_triplet(&bwrap, "--bind", "/repo", "/workspace"));
    assert!(contains_ro_bind_stub(&bwrap, "/etc/profile"));
    assert!(contains_ro_bind_stub(&bwrap, "/etc/bash.bashrc"));
    assert!(contains_arg_pair(&bwrap, "--tmpfs", "/etc/profile.d"));
    assert!(contains_arg_pair(&bwrap, "--chdir", "/workspace"));
    assert!(contains_arg_pair(&bwrap, "--listen", socket.to_str().unwrap_or_default()));
    assert_eq!(bwrap.last().map(String::as_str), Some("/ctx/bin/tsh"));
}

#[test]
fn agent_start_systemd_command_uses_sanitized_environment() {
    let root = PathBuf::from("/ctx");
    let args = AgentStartArgs {
        name: "coder".to_owned(),
        session: "test".to_owned(),
        cwd: "/workspace".to_owned(),
        default_workspace: true,
        mounts: Vec::new(),
    };
    let socket = PathBuf::from("/ctx/home/1000/agent/coder/session/test/terminal/main.sock");
    let command = agent_start_systemd_command(
        &root,
        &args,
        &args.mounts,
        &socket,
        "cortexfs-agent-coder-test-terminal",
    );
    assert!(matches!(
        command,
        Ok(ref command)
            if command.program == "systemd-run"
                && command.args.contains(&"--user".to_owned())
                && contains_arg_pair(&command.args, "--property", "Restart=always")
                && contains_arg_pair(&command.args, "--property", "RestartSec=250ms")
                && command.args.contains(&"-i".to_owned())
                && command.args.contains(&"PATH=/usr/bin:/bin".to_owned())
                && command.args.contains(&"CTX_ROOT=/ctx".to_owned())
                && command.args.contains(&"CTX_HOME=/ctx/home/1000".to_owned())
                && command.args.contains(&"HOME=/workspace".to_owned())
                && command.args.contains(&"USER=coder".to_owned())
                && command.args.contains(&"LOGNAME=coder".to_owned())
                && command.args.contains(&"SHELL=/usr/bin/bash".to_owned())
                && command.args.contains(&"TERM=xterm-256color".to_owned())
                && command.args.contains(&"LANG=C.UTF-8".to_owned())
                && command.args.contains(&"/usr/bin/bwrap".to_owned())
                && !command.args.contains(&"CTX_PATH".to_owned())
    ));
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
fn cli_names_reject_usage_placeholders() {
    assert!(require_cli_name("agent name", "coder").is_ok());
    assert!(matches!(
        require_cli_name("agent name", "NAME"),
        Err(ref error) if error.code == 2
            && error.message == "agent name is a placeholder; replace NAME with a real value"
    ));
    assert!(matches!(
        require_session_name("SESSION"),
        Err(ref error) if error.code == 2
            && error.message
                == "session name is a placeholder; replace SESSION with a real value"
    ));
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
fn tool_command_executes_visible_tool_without_tsh() {
    let root = clean_test_dir("ctx-tool-command-visible");
    let tool = root.join("tool").join("project.echo");
    write_text_file(&tool, "#!/bin/sh\nexit 7\n");
    assert!(fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).is_ok());

    let result = run_visible_tool(&root, "project.echo", &["hello".to_owned()]);
    assert!(matches!(result, Ok(code) if format!("{code:?}") == "ExitCode(unix_exit_status(7))"));
}

#[test]
fn abi_path_resolution_rejects_escape() {
    let root = Path::new("/ctx");
    assert!(resolve_abi_path(root, "agent/coder.d/cwd").is_ok());
    assert!(resolve_abi_path(root, "../etc/passwd").is_err());
    assert!(resolve_abi_path(root, "/etc/passwd").is_err());
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
